// SPDX-License-Identifier: MIT

use super::render::{PreparedMaterial, digest, item_ref, render};
use super::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DraftRecord {
    draft: Draft,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PreviewRecord {
    preview: Preview,
    material: Option<PreparedMaterial>,
    consumed: bool,
}

/// Harness-facing control authority used by the synthetic console and by tests.  The target
/// adapter can inspect this state, but only this object changes pause, revision and plan epochs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControlPlane {
    scope: Scope,
    enabled: bool,
    now: u64,
    boundary: Boundary,
    state: State,
    #[serde(skip)]
    items: BTreeMap<(String, u64), ItemRecord>,
    latest_versions: BTreeMap<String, u64>,
    drafts: BTreeMap<String, DraftRecord>,
    revisions: BTreeMap<String, Revision>,
    previews: BTreeMap<String, PreviewRecord>,
    commands: BTreeMap<String, (String, Receipt)>,
    events: Vec<Event>,
    relations: Vec<Relation>,
    next_id: u64,
    continuation_preview_id: Option<String>,
    prepared_input: Option<Vec<u8>>,
}

impl ControlPlane {
    pub fn synthetic() -> Self {
        let scope = Scope::new(
            "fixture-project",
            "fixture-run",
            "fixture-episode",
            "fixture-agent",
        )
        .expect("synthetic scope is valid");
        Self::new(scope, true, 1_788_998_400).expect("synthetic control plane is valid")
    }

    pub fn new(scope: Scope, enabled: bool, now: u64) -> Result<Self, ControlError> {
        let boundary = Boundary {
            scope: scope.clone(),
            state_id: "fixture-state-1".to_owned(),
            generation: 7,
            observation_sha256: digest(b"fixture-observation-v1"),
            catalog_sha256: digest(b"fixture-catalog-v1"),
            controller_epoch: 1,
            gate_epoch: 0,
            control_version: 0,
            lease_epoch: 1,
            adapter_revision: "fixture-adapter-v1".to_owned(),
            adapter_sha256: digest(b"fixture-adapter-v1"),
            model: "synthetic-fixture-only".to_owned(),
            configuration_sha256: digest(b"legacy-configuration-v1"),
            output_schema_sha256: digest(super::render::output_schema()),
            capabilities_sha256: digest(b"ascension-context-control-capabilities-v1"),
            authorization_policy_version: "policy-1".to_owned(),
        };
        let state = State {
            schema: STATE_SCHEMA.to_owned(),
            scope: scope.clone(),
            status: "running".to_owned(),
            control_version: 0,
            controller_epoch: 1,
            gate_epoch: 0,
            plan_epoch: 1,
            active_revision_id: "revision-1".to_owned(),
            pause_latched: false,
            stop_latched: false,
            outstanding_provider_attempts: Vec::new(),
            unresolved_operations: Vec::new(),
            boundary: Some(boundary.clone()),
            command_window_id: "window-1".to_owned(),
            command_window_expires_at: format_time(now.saturating_add(3600)),
            last_sequence: 0,
        };
        let mut plane = Self {
            scope,
            enabled,
            now,
            boundary,
            state,
            items: BTreeMap::new(),
            latest_versions: BTreeMap::new(),
            drafts: BTreeMap::new(),
            revisions: BTreeMap::new(),
            previews: BTreeMap::new(),
            commands: BTreeMap::new(),
            events: Vec::new(),
            relations: Vec::new(),
            next_id: 1,
            continuation_preview_id: None,
            prepared_input: None,
        };
        plane.seed_items();
        plane.revisions.insert(
            "revision-1".to_owned(),
            Revision {
                schema: REVISION_SCHEMA.to_owned(),
                scope: plane.scope.clone(),
                revision_id: "revision-1".to_owned(),
                sequence: 1,
                parent_revision_id: "revision-1".to_owned(),
                selected_items: Vec::new(),
                pinned_item_ids: Vec::new(),
                note_items: Vec::new(),
                objective_item: None,
                approved_preview_id: "bootstrap".to_owned(),
                approved_manifest_sha256: digest(b"bootstrap-revision"),
                author_ref: "system".to_owned(),
                intervention_id: "intervention-bootstrap".to_owned(),
                committed_at: format_time(now),
                plan_epoch: 1,
                state_after_commit: "paused_committed".to_owned(),
            },
        );
        Ok(plane)
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn capabilities(&self) -> Capabilities {
        let operations = if self.enabled {
            [
                "include_item",
                "exclude_item",
                "pin_item",
                "unpin_item",
                "put_note",
                "remove_note",
                "set_objective",
                "restore_configuration",
                "pause",
                "commit",
                "resume",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        } else {
            Vec::new()
        };
        Capabilities {
            schema: CAPABILITIES_SCHEMA.to_owned(),
            product_phase: 2,
            scope: self.scope.clone(),
            enabled: self.enabled,
            adapter_revision: self.boundary.adapter_revision.clone(),
            supported_operations: operations,
            exact_application_preview: "supported".to_owned(),
            optional_images: "unsupported".to_owned(),
            durable_control_store: "available".to_owned(),
            provider_added_context: "not_exposed".to_owned(),
            context_compact: false,
            persistent_provider_sessions: false,
            direct_game_dispatch: false,
            commit_auto_resumes: false,
        }
    }

    pub fn state(&self) -> State {
        self.state.clone()
    }

    pub fn eligible_items(&self) -> Vec<EligibleItem> {
        self.items
            .values()
            .map(|record| EligibleItem {
                item: record.item.clone(),
                kind: record.kind.clone(),
                protected: record.protected,
                scope: record.scope.clone(),
                content_available: !record.content.is_empty() && !record.protected,
                bytes: record.content.len(),
                expires_at: record.expires_text.clone(),
                locked_reason: record.locked_reason.clone(),
                content: (!record.protected)
                    .then(|| String::from_utf8_lossy(&record.content).into_owned()),
            })
            .collect()
    }

    pub fn drafts(&self) -> Vec<Draft> {
        self.drafts
            .values()
            .map(|record| record.draft.clone())
            .collect()
    }

    pub fn revisions(&self) -> Vec<Revision> {
        self.revisions.values().cloned().collect()
    }

    pub fn events(&self) -> Vec<Event> {
        self.events.clone()
    }

    pub fn relations(&self) -> Vec<Relation> {
        self.relations.clone()
    }

    pub fn prepared_input(&self) -> Option<&[u8]> {
        self.prepared_input.as_deref()
    }

    pub fn export_journal(&self) -> Result<Vec<u8>, ControlError> {
        #[derive(Serialize)]
        struct Journal<'a> {
            schema: &'static str,
            plane: &'a ControlPlane,
            items: Vec<((String, u64), ItemRecord)>,
        }
        serde_json::to_vec(&Journal {
            schema: JOURNAL_SCHEMA,
            plane: self,
            items: self
                .items
                .iter()
                .map(|(key, item)| (key.clone(), item.clone()))
                .collect(),
        })
        .map_err(|_| ControlError::invalid("journal_encode", "control journal cannot be encoded"))
    }

    pub fn recover_journal(bytes: &[u8]) -> Result<Self, ControlError> {
        #[derive(Deserialize)]
        struct Journal {
            schema: String,
            plane: ControlPlane,
            items: Vec<((String, u64), ItemRecord)>,
        }
        let mut journal: Journal = serde_json::from_slice(bytes)
            .map_err(|_| ControlError::invalid("journal_decode", "control journal is invalid"))?;
        if journal.schema != JOURNAL_SCHEMA || journal.plane.events.len() > MAX_EVENTS {
            return Err(ControlError::invalid(
                "journal_invalid",
                "control journal exceeds its bound",
            ));
        }
        for (key, item) in journal.items {
            journal.plane.items.insert(key, item);
        }
        journal.plane.state.controller_epoch =
            journal.plane.state.controller_epoch.saturating_add(1);
        journal.plane.boundary.controller_epoch = journal.plane.state.controller_epoch;
        journal.plane.sync_boundary();
        Ok(journal.plane)
    }

    pub fn get_draft(&self, draft_id: &str) -> Result<Draft, ControlError> {
        self.drafts
            .get(draft_id)
            .map(|record| record.draft.clone())
            .ok_or_else(|| ControlError::invalid("draft_not_found", "draft is unavailable"))
    }

    pub fn get_preview(&self, preview_id: &str) -> Result<Preview, ControlError> {
        self.previews
            .get(preview_id)
            .map(|record| record.preview.clone())
            .ok_or_else(|| ControlError::invalid("preview_not_found", "preview is unavailable"))
    }

    pub fn create_draft(
        &mut self,
        scope: Scope,
        expected_active_revision_id: &str,
        author_ref: &str,
    ) -> Result<Draft, ControlError> {
        self.require_enabled()?;
        self.check_scope(&scope)?;
        self.check_active_revision(expected_active_revision_id)?;
        if !valid_id(author_ref) {
            return Err(ControlError::invalid(
                "invalid_author",
                "author reference is invalid",
            ));
        }
        let draft_id = self.id("draft");
        let draft = Draft {
            schema: DRAFT_SCHEMA.to_owned(),
            scope: scope.clone(),
            draft_id: draft_id.clone(),
            version: 1,
            base_revision_id: expected_active_revision_id.to_owned(),
            selected_items: Vec::new(),
            pinned_item_ids: Vec::new(),
            note_items: Vec::new(),
            objective_item: None,
            author_ref: author_ref.to_owned(),
            expires_at: format_time(self.now.saturating_add(3600)),
        };
        self.drafts.insert(
            draft_id,
            DraftRecord {
                draft: draft.clone(),
            },
        );
        self.record_event("draft.updated", None, None, None, Some("draft_created"));
        Ok(draft)
    }

    pub fn apply_patch(
        &mut self,
        patch: Patch,
        author: &str,
        objective_authorized: bool,
    ) -> Result<Draft, ControlError> {
        self.require_enabled()?;
        if patch.schema != PATCH_SCHEMA
            || patch.operations.is_empty()
            || patch.operations.len() > MAX_OPERATIONS
        {
            return Err(ControlError::invalid(
                "invalid_patch",
                "patch shape is invalid",
            ));
        }
        self.check_scope(&patch.scope)?;
        self.check_active_revision(&patch.expected_active_revision_id)?;
        if !valid_id(author) {
            return Err(ControlError::invalid(
                "invalid_author",
                "author reference is invalid",
            ));
        }
        let record = self
            .drafts
            .get(&patch.draft_id)
            .ok_or_else(|| ControlError::invalid("draft_not_found", "draft is unavailable"))?;
        if record.draft.version != patch.expected_draft_version {
            return Err(ControlError::conflict(
                "stale_draft",
                "draft version is stale",
            ));
        }
        if record.draft.scope != patch.scope {
            return Err(ControlError::forbidden(
                "scope_mismatch",
                "draft scope does not match",
            ));
        }
        let mut draft = record.draft.clone();
        let restore = patch
            .operations
            .iter()
            .any(|operation| matches!(operation, Operation::RestoreConfiguration { .. }));
        if restore && patch.operations.len() != 1 {
            return Err(ControlError::invalid(
                "restore_must_be_alone",
                "restore cannot be combined with edits",
            ));
        }
        for operation in patch.operations {
            self.apply_operation(&mut draft, operation, objective_authorized)?;
        }
        draft.version = draft
            .version
            .checked_add(1)
            .ok_or_else(|| ControlError::invalid("version_overflow", "draft version overflowed"))?;
        draft.author_ref = author.to_owned();
        self.drafts.insert(
            patch.draft_id.clone(),
            DraftRecord {
                draft: draft.clone(),
            },
        );
        self.invalidate_previews(&patch.draft_id, "draft_changed");
        self.record_event("draft.updated", None, None, None, Some("draft_saved"));
        Ok(draft)
    }

    pub fn create_preview(
        &mut self,
        scope: Scope,
        draft_id: &str,
        expected_draft_version: u64,
        applicable_requested: bool,
        expected_control_version: u64,
        risk_ack: bool,
    ) -> Result<Preview, ControlError> {
        self.require_enabled()?;
        self.check_scope(&scope)?;
        if self.state.control_version != expected_control_version {
            return Err(ControlError::conflict(
                "stale_control",
                "control version is stale",
            ));
        }
        let draft = self.get_draft(draft_id)?;
        if draft.version != expected_draft_version {
            return Err(ControlError::conflict(
                "stale_draft",
                "draft version is stale",
            ));
        }
        let preview_id = self.id("preview");
        let held = self.state.pause_latched
            && self.state.status == "paused_ready"
            && self.state.unresolved_operations.is_empty();
        let applicable = applicable_requested && held;
        let mut blockers = Vec::new();
        if applicable_requested && !held {
            blockers.push("run_not_held".to_owned());
        }
        let material = if blockers.is_empty() {
            render(&preview_id, &self.boundary, &draft, &self.items).ok()
        } else {
            None
        };
        if material.is_none() && blockers.is_empty() {
            blockers.push("mandatory_budget_exceeded".to_owned());
        }
        let expiry = format_time(self.now.saturating_add(120));
        let preview = Preview {
            schema: PREVIEW_SCHEMA.to_owned(),
            preview_id: preview_id.clone(),
            scope: scope.clone(),
            draft_id: draft_id.to_owned(),
            draft_version: draft.version,
            base_revision_id: draft.base_revision_id.clone(),
            boundary: self.boundary.clone(),
            applicable: applicable && material.is_some(),
            blockers,
            prepared_manifest_sha256: material.as_ref().map(|value| value.manifest_sha256.clone()),
            components: material
                .as_ref()
                .map_or_else(Vec::new, |value| value.components.clone()),
            selected_items: draft.selected_items.clone(),
            budget_status: "within_known_local_limit".to_owned(),
            unknown_total_risk_acknowledged: risk_ack,
            provider_added_context: "not_exposed".to_owned(),
            model_execution_id: material.as_ref().map(|_| self.id("execution")),
            provider_attempt_id: material.as_ref().map(|_| self.id("attempt")),
            expires_at: expiry,
            effect_class: "local_preparation_only".to_owned(),
        };
        self.previews.insert(
            preview_id,
            PreviewRecord {
                preview: preview.clone(),
                material,
                consumed: false,
            },
        );
        self.record_event("preview.built", None, Some(&preview.preview_id), None, None);
        Ok(preview)
    }

    pub fn pause(&mut self, command: Command) -> Result<Receipt, ControlError> {
        self.require_enabled()?;
        if let Some(receipt) = self.idempotent_result(&command)? {
            return Ok(receipt);
        }
        self.command_guard(&command, "pause", None, None)?;
        if self.state.stop_latched || self.state.pause_latched {
            return Err(ControlError::conflict(
                "already_paused",
                "run is already paused or stopped",
            ));
        }
        self.state.pause_latched = true;
        self.state.gate_epoch = self.state.gate_epoch.saturating_add(1);
        self.state.control_version = self.state.control_version.saturating_add(1);
        self.sync_boundary();
        self.state.status = if self.quiescent() {
            "paused_ready"
        } else {
            "pause_requested"
        }
        .to_owned();
        let receipt = self.receipt(&command, "pause_requested", None);
        self.save_command(&command, &receipt);
        self.record_event(
            "pause.accepted",
            Some(&receipt.command_id),
            None,
            None,
            None,
        );
        if self.state.status == "paused_ready" {
            self.record_event("pause.ready", Some(&receipt.command_id), None, None, None);
        }
        Ok(receipt)
    }

    /// Host recovery can latch a durable stop independently of the operator command window.
    /// Stop dominates every later resume attempt and leaves the active revision untouched.
    pub fn stop(&mut self) {
        self.state.stop_latched = true;
        self.state.pause_latched = true;
        self.state.status = "stopped".to_owned();
        self.state.gate_epoch = self.state.gate_epoch.saturating_add(1);
        self.state.control_version = self.state.control_version.saturating_add(1);
        self.sync_boundary();
        self.record_event(
            "stop.latched",
            None,
            None,
            None,
            Some("stop_dominates_resume"),
        );
    }

    pub fn commit(&mut self, command: Command) -> Result<Receipt, ControlError> {
        self.require_enabled()?;
        if let Some(receipt) = self.idempotent_result(&command)? {
            return Ok(receipt);
        }
        self.command_guard(
            &command,
            "commit",
            command.expected_active_revision_id.as_deref(),
            command.preview_id.as_deref(),
        )?;
        if !self.state.pause_latched
            || !matches!(self.state.status.as_str(), "paused_ready" | "paused_stale")
        {
            return Err(ControlError::conflict(
                "run_not_ready",
                "commit requires a held ready boundary",
            ));
        }
        if !self.quiescent() {
            return Err(ControlError::conflict(
                "unresolved_operation",
                "commit is blocked by an unresolved effect",
            ));
        }
        let preview_id = command.preview_id.as_deref().ok_or_else(|| {
            ControlError::invalid("preview_required", "applicable preview is required")
        })?;
        let preview = self
            .previews
            .get(preview_id)
            .ok_or_else(|| ControlError::invalid("preview_not_found", "preview is unavailable"))?
            .clone();
        if preview.consumed
            || !preview.preview.applicable
            || preview.preview.prepared_manifest_sha256.as_deref()
                != command.approved_manifest_sha256.as_deref()
            || preview.preview.boundary != self.boundary
        {
            return Err(ControlError::conflict(
                "preview_stale",
                "preview approval is stale",
            ));
        }
        let draft = self.get_draft(&preview.preview.draft_id)?;
        let revision_id = self.id("revision");
        let intervention_id = self.id("intervention");
        let revision = Revision {
            schema: REVISION_SCHEMA.to_owned(),
            scope: self.scope.clone(),
            revision_id: revision_id.clone(),
            sequence: self.revisions.len() as u64 + 1,
            parent_revision_id: self.state.active_revision_id.clone(),
            selected_items: draft.selected_items.clone(),
            pinned_item_ids: draft.pinned_item_ids.clone(),
            note_items: draft.note_items.clone(),
            objective_item: draft.objective_item.clone(),
            approved_preview_id: preview.preview.preview_id.clone(),
            approved_manifest_sha256: preview
                .preview
                .prepared_manifest_sha256
                .clone()
                .unwrap_or_default(),
            author_ref: draft.author_ref.clone(),
            intervention_id: intervention_id.clone(),
            committed_at: format_time(self.now),
            plan_epoch: self.state.plan_epoch.saturating_add(1),
            state_after_commit: "paused_committed".to_owned(),
        };
        self.revisions.insert(revision_id.clone(), revision);
        if let Some(entry) = self.previews.get_mut(preview_id) {
            entry.consumed = true;
        }
        self.state.active_revision_id = revision_id.clone();
        self.state.plan_epoch = self.state.plan_epoch.saturating_add(1);
        self.state.control_version = self.state.control_version.saturating_add(1);
        self.state.status = "paused_committed".to_owned();
        self.sync_boundary();
        self.continuation_preview_id = Some(preview_id.to_owned());
        self.prepared_input = preview.material.map(|material| material.input);
        let receipt = self.receipt(&command, "revision_committed", None);
        self.save_command(&command, &receipt);
        self.record_event(
            "revision.committed",
            Some(&receipt.command_id),
            Some(preview_id),
            Some(&revision_id),
            None,
        );
        self.record_event(
            "plan.retired",
            Some(&receipt.command_id),
            None,
            Some(&revision_id),
            Some("plan_epoch_advanced"),
        );
        let relation_id = self.id("relation");
        self.relations.push(Relation {
            schema: RELATION_SCHEMA.to_owned(),
            scope: self.scope.clone(),
            relation_id,
            revision_id,
            preview_id: Some(preview_id.to_owned()),
            phase1_snapshot_id: "snapshot-fixture-cli-001".to_owned(),
            model_execution_id: preview
                .preview
                .model_execution_id
                .unwrap_or_else(|| "execution-unavailable".to_owned()),
            provider_attempt_id: preview
                .preview
                .provider_attempt_id
                .unwrap_or_else(|| "attempt-unavailable".to_owned()),
            plan_id: None,
            action_id: None,
            intervention_id: Some(intervention_id),
            first_approved_input: true,
            evidence: "synthetic".to_owned(),
        });
        Ok(receipt)
    }

    pub fn resume(&mut self, command: Command) -> Result<Receipt, ControlError> {
        self.require_enabled()?;
        if let Some(receipt) = self.idempotent_result(&command)? {
            return Ok(receipt);
        }
        self.command_guard(
            &command,
            "resume",
            command.expected_active_revision_id.as_deref(),
            command.expected_preview_id.as_deref(),
        )?;
        if self.state.stop_latched {
            return Err(ControlError::forbidden(
                "stopped",
                "stop intent dominates resume",
            ));
        }
        if !self.state.pause_latched || !self.quiescent() {
            return Err(ControlError::conflict(
                "not_ready",
                "resume requires a quiescent paused boundary",
            ));
        }
        if let Some(expected) = command.expected_preview_id.as_deref()
            && self.continuation_preview_id.as_deref() != Some(expected)
        {
            return Err(ControlError::conflict(
                "preview_stale",
                "approved continuation is unavailable",
            ));
        }
        if let Some(preview_id) = self.continuation_preview_id.as_deref() {
            let continuation = self.previews.get(preview_id).ok_or_else(|| {
                ControlError::conflict("preview_stale", "approved continuation is unavailable")
            })?;
            if !same_external_boundary(&continuation.preview.boundary, &self.boundary) {
                return Err(ControlError::conflict(
                    "preview_stale",
                    "approved continuation boundary is stale",
                ));
            }
        }
        self.state.pause_latched = false;
        self.state.gate_epoch = self.state.gate_epoch.saturating_add(1);
        self.state.control_version = self.state.control_version.saturating_add(1);
        self.state.status = "running".to_owned();
        self.sync_boundary();
        let receipt = self.receipt(&command, "resume_accepted", None);
        self.save_command(&command, &receipt);
        let continuation = self.continuation_preview_id.clone();
        self.record_event(
            "resume.accepted",
            Some(&receipt.command_id),
            continuation.as_deref(),
            None,
            None,
        );
        if self.prepared_input.is_some() {
            self.record_event(
                "input.claimed",
                Some(&receipt.command_id),
                continuation.as_deref(),
                None,
                None,
            );
            self.record_event(
                "input.submitted",
                Some(&receipt.command_id),
                continuation.as_deref(),
                None,
                None,
            );
        }
        self.continuation_preview_id = None;
        Ok(receipt)
    }

    pub fn command(&self, command_id: &str) -> Result<Receipt, ControlError> {
        self.commands
            .values()
            .find_map(|(_, receipt)| (receipt.command_id == command_id).then_some(receipt.clone()))
            .ok_or_else(|| ControlError::invalid("command_not_found", "command is unavailable"))
    }

    pub fn advance_boundary(&mut self) {
        self.boundary.generation = self.boundary.generation.saturating_add(1);
        self.boundary.observation_sha256 =
            digest(format!("observation-{}", self.boundary.generation).as_bytes());
        self.state.boundary = Some(self.boundary.clone());
        if self.state.pause_latched {
            self.state.status = "paused_stale".to_owned();
        }
        self.invalidate_all_previews("boundary_changed");
    }

    pub fn validate_plan_epoch(&self, epoch: u64) -> Result<(), ControlError> {
        (epoch == self.state.plan_epoch && !self.state.pause_latched && !self.state.stop_latched)
            .then_some(())
            .ok_or_else(|| {
                ControlError::conflict("obsolete_plan", "plan epoch is no longer dispatchable")
            })
    }

    fn seed_items(&mut self) {
        self.insert_item(
            "history-1",
            1,
            "history",
            false,
            b"A bounded historical context artifact retained for this fixture.\n",
            4_102_444_800,
            None,
        );
        self.insert_item(
            "note-1",
            1,
            "note",
            false,
            b"Prefer preserving health when several legal choices are available.\n",
            4_102_444_800,
            None,
        );
        self.insert_item(
            "objective-1",
            1,
            "objective",
            false,
            b"Preserve the run while choosing only visible legal actions.",
            4_102_444_800,
            None,
        );
        self.insert_item(
            "locked-state",
            1,
            "protected_state",
            true,
            br#"{"state":"host-owned"}"#,
            4_102_444_800,
            Some("host-owned state and legal catalog cannot be edited"),
        );
    }

    // The seed and versioned-note paths intentionally share one bounded registry insertion
    // boundary; keeping all fields at this call site makes scope, expiry, and protection reviewable.
    #[allow(clippy::too_many_arguments)]
    fn insert_item(
        &mut self,
        id: &str,
        version: u64,
        kind: &str,
        protected: bool,
        content: &[u8],
        expires_at: u64,
        locked_reason: Option<&str>,
    ) {
        let item = item_ref(id, version, content);
        self.latest_versions.insert(id.to_owned(), version);
        self.items.insert(
            (id.to_owned(), version),
            ItemRecord {
                item,
                kind: kind.to_owned(),
                protected,
                scope: self.scope.clone(),
                content: content.to_vec(),
                expires_at,
                expires_text: format_time(expires_at),
                locked_reason: locked_reason.map(str::to_owned),
            },
        );
    }

    fn apply_operation(
        &mut self,
        draft: &mut Draft,
        operation: Operation,
        objective_authorized: bool,
    ) -> Result<(), ControlError> {
        match operation {
            Operation::IncludeItem { item } => {
                let record = self.editable_item(&item)?;
                if draft.selected_items.len() >= MAX_ITEMS {
                    return Err(ControlError::invalid(
                        "item_limit",
                        "selected item limit is reached",
                    ));
                }
                if draft
                    .selected_items
                    .iter()
                    .any(|current| current.item_id == item.item_id)
                {
                    return Err(ControlError::conflict(
                        "duplicate_selected_item",
                        "item is already selected",
                    ));
                }
                draft.selected_items.push(record.item.clone());
            }
            Operation::ExcludeItem { item } => {
                self.editable_item(&item)?;
                if draft.pinned_item_ids.iter().any(|id| id == &item.item_id) {
                    return Err(ControlError::forbidden(
                        "pinned_item",
                        "unpin the item before exclusion",
                    ));
                }
                draft
                    .selected_items
                    .retain(|current| current.item_id != item.item_id);
                draft
                    .note_items
                    .retain(|current| current.item_id != item.item_id);
            }
            Operation::PinItem { item } => {
                self.editable_item(&item)?;
                if !draft.selected_items.iter().any(|current| current == &item) {
                    return Err(ControlError::invalid(
                        "pin_requires_selection",
                        "pinned item must be selected",
                    ));
                }
                if !draft.pinned_item_ids.contains(&item.item_id) {
                    draft.pinned_item_ids.push(item.item_id);
                }
            }
            Operation::UnpinItem { item } => {
                self.editable_item(&item)?;
                draft.pinned_item_ids.retain(|id| id != &item.item_id);
            }
            Operation::PutNote {
                note_id,
                expected_note_version,
                text,
                expires_at,
            } => {
                self.put_note(draft, &note_id, expected_note_version, text, &expires_at)?;
            }
            Operation::RemoveNote {
                note_id,
                expected_note_version,
            } => {
                let current = draft
                    .note_items
                    .iter()
                    .find(|item| item.item_id == note_id)
                    .ok_or_else(|| {
                        ControlError::invalid("note_not_found", "note is unavailable")
                    })?;
                if current.version != expected_note_version {
                    return Err(ControlError::conflict(
                        "stale_note",
                        "note version is stale",
                    ));
                }
                draft.note_items.retain(|item| item.item_id != note_id);
                draft.selected_items.retain(|item| item.item_id != note_id);
                draft.pinned_item_ids.retain(|id| id != &note_id);
            }
            Operation::SetObjective { text } => {
                if !objective_authorized {
                    return Err(ControlError::forbidden(
                        "objective_authorization_required",
                        "objective override requires the objective scope",
                    ));
                }
                if text.is_empty() || text.len() > MAX_OBJECTIVE_BYTES || text.contains('\0') {
                    return Err(ControlError::invalid(
                        "objective_bounds",
                        "objective exceeds its UTF-8 bound",
                    ));
                }
                let version = self
                    .latest_versions
                    .get("objective-override")
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(1);
                self.insert_item(
                    "objective-override",
                    version,
                    "objective",
                    false,
                    text.as_bytes(),
                    self.now.saturating_add(3600),
                    None,
                );
                draft.objective_item = self
                    .items
                    .get(&("objective-override".to_owned(), version))
                    .map(|record| record.item.clone());
            }
            Operation::RestoreConfiguration { source_revision_id } => {
                let revision = self
                    .revisions
                    .get(&source_revision_id)
                    .ok_or_else(|| {
                        ControlError::invalid(
                            "revision_not_found",
                            "source revision is unavailable",
                        )
                    })?
                    .clone();
                draft.selected_items = revision.selected_items;
                draft.pinned_item_ids = revision.pinned_item_ids;
                draft.note_items = revision.note_items;
                draft.objective_item = revision.objective_item;
            }
        }
        Ok(())
    }

    fn put_note(
        &mut self,
        draft: &mut Draft,
        note_id: &str,
        expected: Option<u64>,
        text: String,
        expires_at: &str,
    ) -> Result<(), ControlError> {
        if !valid_id(note_id)
            || text.is_empty()
            || text.len() > MAX_NOTE_BYTES
            || text.contains('\0')
        {
            return Err(ControlError::invalid(
                "note_bounds",
                "note is outside its UTF-8 bound",
            ));
        }
        let expiry = parse_time(expires_at)
            .ok_or_else(|| ControlError::invalid("invalid_expiry", "note expiry is invalid"))?;
        if expiry <= self.now {
            return Err(ControlError::conflict(
                "expired_content",
                "note expiry is in the past",
            ));
        }
        let current = draft
            .note_items
            .iter()
            .find(|item| item.item_id == note_id)
            .map(|item| item.version);
        if current != expected {
            return Err(ControlError::conflict(
                "stale_note",
                "expected note version does not match",
            ));
        }
        let version = current
            .unwrap_or_else(|| self.latest_versions.get(note_id).copied().unwrap_or(0))
            .saturating_add(1);
        if draft.note_items.len() >= MAX_NOTES {
            return Err(ControlError::invalid("note_limit", "note limit is reached"));
        }
        let total_note_bytes = draft
            .note_items
            .iter()
            .filter_map(|item| self.items.get(&(item.item_id.clone(), item.version)))
            .map(|record| record.content.len())
            .sum::<usize>();
        if total_note_bytes.saturating_add(text.len()) > MAX_TOTAL_NOTE_BYTES {
            return Err(ControlError::invalid(
                "note_budget",
                "aggregate note budget is exceeded",
            ));
        }
        self.insert_item(
            note_id,
            version,
            "note",
            false,
            text.as_bytes(),
            expiry,
            None,
        );
        let reference = self
            .items
            .get(&(note_id.to_owned(), version))
            .map(|record| record.item.clone())
            .ok_or_else(|| {
                ControlError::invalid("note_unavailable", "note could not be retained")
            })?;
        draft.note_items.retain(|item| item.item_id != note_id);
        draft.note_items.push(reference.clone());
        draft.selected_items.retain(|item| item.item_id != note_id);
        draft.selected_items.push(reference);
        Ok(())
    }

    fn editable_item(&self, item: &ItemRef) -> Result<&ItemRecord, ControlError> {
        if !item.valid() {
            return Err(ControlError::invalid(
                "invalid_item",
                "item reference is invalid",
            ));
        }
        let record = self
            .items
            .get(&(item.item_id.clone(), item.version))
            .ok_or_else(|| ControlError::invalid("unknown_item", "item version is unavailable"))?;
        if record.item != *item {
            return Err(ControlError::conflict(
                "item_digest_mismatch",
                "item digest does not match retained bytes",
            ));
        }
        if record.scope != self.scope {
            return Err(ControlError::forbidden(
                "scope_mismatch",
                "item is outside this run",
            ));
        }
        if record.protected {
            return Err(ControlError::forbidden(
                "protected_item",
                record
                    .locked_reason
                    .clone()
                    .unwrap_or_else(|| "item is host-owned".to_owned()),
            ));
        }
        if record.expires_at <= self.now || record.content.is_empty() {
            return Err(ControlError::conflict(
                "content_unavailable",
                "editable content is expired or unavailable",
            ));
        }
        Ok(record)
    }

    fn command_guard(
        &self,
        command: &Command,
        kind: &str,
        expected_revision: Option<&str>,
        expected_preview: Option<&str>,
    ) -> Result<(), ControlError> {
        if command.schema != COMMAND_SCHEMA
            || command.kind != kind
            || !valid_id(&command.idempotency_key)
            || !valid_id(&command.command_window_id)
        {
            return Err(ControlError::invalid(
                "invalid_command",
                "command shape is invalid",
            ));
        }
        self.check_scope(&command.scope)?;
        if command.command_window_id != self.state.command_window_id
            || self.now >= parse_time(&self.state.command_window_expires_at).unwrap_or(0)
        {
            return Err(ControlError::forbidden(
                "command_window_expired",
                "management command window is expired",
            ));
        }
        if let Some(expected) = expected_revision
            && (command.expected_active_revision_id.as_deref() != Some(expected)
                || self.state.active_revision_id != expected)
        {
            return Err(ControlError::conflict(
                "stale_revision",
                "active revision is stale",
            ));
        }
        if let Some(expected) = expected_preview
            && command.expected_preview_id.as_deref() != Some(expected)
            && command.preview_id.as_deref() != Some(expected)
        {
            return Err(ControlError::conflict(
                "preview_mismatch",
                "preview identity is stale",
            ));
        }
        if command.expected_control_version != self.state.control_version {
            return Err(ControlError::conflict(
                "stale_control",
                "control version is stale",
            ));
        }
        Ok(())
    }

    fn idempotent_result(&self, command: &Command) -> Result<Option<Receipt>, ControlError> {
        let Some((stored_digest, receipt)) = self.commands.get(&command.idempotency_key) else {
            return Ok(None);
        };
        let current = serde_json::to_vec(command)
            .map(|bytes| digest(&bytes))
            .map_err(|_| ControlError::invalid("invalid_command", "command cannot be encoded"))?;
        if &current == stored_digest {
            return Ok(Some(receipt.clone()));
        }
        Err(ControlError::conflict(
            "idempotency_conflict",
            "idempotency key was reused with a different command",
        ))
    }

    fn receipt(&mut self, command: &Command, effect: &str, reason_code: Option<&str>) -> Receipt {
        Receipt {
            schema: RECEIPT_SCHEMA.to_owned(),
            command_id: self.id(&format!("command-{}", command.kind)),
            scope: self.scope.clone(),
            kind: command.kind.clone(),
            status: "completed".to_owned(),
            effect: effect.to_owned(),
            control_version: self.state.control_version,
            active_revision_id: self.state.active_revision_id.clone(),
            paused: self.state.pause_latched,
            reason_code: reason_code.map(str::to_owned),
            observed_at: format_time(self.now),
        }
    }

    fn save_command(&mut self, command: &Command, receipt: &Receipt) {
        let digest = serde_json::to_vec(command)
            .map(|bytes| digest(&bytes))
            .unwrap_or_default();
        self.commands
            .insert(command.idempotency_key.clone(), (digest, receipt.clone()));
    }

    fn check_scope(&self, scope: &Scope) -> Result<(), ControlError> {
        scope.same(&self.scope).then_some(()).ok_or_else(|| {
            ControlError::forbidden("scope_mismatch", "request scope is outside this run")
        })
    }
    fn check_active_revision(&self, revision: &str) -> Result<(), ControlError> {
        (revision == self.state.active_revision_id)
            .then_some(())
            .ok_or_else(|| {
                ControlError::conflict("stale_revision", "active revision does not match")
            })
    }
    fn require_enabled(&self) -> Result<(), ControlError> {
        self.enabled.then_some(()).ok_or_else(|| {
            ControlError::forbidden("management_disabled", "context management is disabled")
        })
    }
    fn quiescent(&self) -> bool {
        self.state.outstanding_provider_attempts.is_empty()
            && self.state.unresolved_operations.is_empty()
    }
    fn sync_boundary(&mut self) {
        self.boundary.control_version = self.state.control_version;
        self.boundary.controller_epoch = self.state.controller_epoch;
        self.boundary.gate_epoch = self.state.gate_epoch;
        self.state.boundary = Some(self.boundary.clone());
    }
    fn invalidate_previews(&mut self, draft_id: &str, reason: &str) {
        for record in self
            .previews
            .values_mut()
            .filter(|record| record.preview.draft_id == draft_id && !record.consumed)
        {
            record.preview.applicable = false;
            record.preview.blockers = vec![reason.to_owned()];
            record.preview.prepared_manifest_sha256 = None;
            record.material = None;
        }
    }
    fn invalidate_all_previews(&mut self, reason: &str) {
        for record in self.previews.values_mut().filter(|record| !record.consumed) {
            record.preview.applicable = false;
            record.preview.blockers = vec![reason.to_owned()];
            record.preview.prepared_manifest_sha256 = None;
            record.material = None;
        }
    }
    fn record_event(
        &mut self,
        event_type: &str,
        command_id: Option<&str>,
        preview_id: Option<&str>,
        revision_id: Option<&str>,
        reason_code: Option<&str>,
    ) {
        self.state.last_sequence = self.state.last_sequence.saturating_add(1);
        if self.events.len() >= MAX_EVENTS {
            return;
        }
        let event_id = self.id("event");
        self.events.push(Event {
            schema: EVENT_SCHEMA.to_owned(),
            event_id,
            scope: self.scope.clone(),
            sequence: self.state.last_sequence,
            event_type: event_type.to_owned(),
            command_id: command_id.map(str::to_owned),
            revision_id: revision_id.map(str::to_owned),
            preview_id: preview_id.map(str::to_owned),
            phase1_snapshot_id: Some("snapshot-fixture-cli-001".to_owned()),
            model_execution_id: None,
            provider_attempt_id: None,
            action_id: None,
            reason_code: reason_code.map(str::to_owned),
            observed_at: format_time(self.now),
        });
    }
    fn id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

pub(crate) fn parse_time(value: &str) -> Option<u64> {
    let date = value.strip_suffix('Z')?;
    let (day, time) = date.split_once('T')?;
    let mut parts = day.split('-');
    let year = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let day = parts.next()?.parse::<i64>().ok()?;
    let mut clock = time.split(':');
    let hour = clock.next()?.parse::<u64>().ok()?;
    let minute = clock.next()?.parse::<u64>().ok()?;
    let second = clock.next()?.split('.').next()?.parse::<u64>().ok()?;
    let days = days_from_civil(year, month, day)?;
    Some(
        (days as u64)
            .saturating_mul(86_400)
            .saturating_add(hour.saturating_mul(3600))
            .saturating_add(minute.saturating_mul(60))
            .saturating_add(second),
    )
}

pub(crate) fn format_time(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}

/// Compare the parts of a boundary supplied by the harness observation and prepared-input
/// contract. `control_version` is deliberately omitted because accepting a commit increments
/// that local CAS version while the continuation remains valid for the same observed boundary.
fn same_external_boundary(left: &Boundary, right: &Boundary) -> bool {
    left.scope == right.scope
        && left.state_id == right.state_id
        && left.generation == right.generation
        && left.observation_sha256 == right.observation_sha256
        && left.catalog_sha256 == right.catalog_sha256
        && left.controller_epoch == right.controller_epoch
        && left.gate_epoch == right.gate_epoch
        && left.lease_epoch == right.lease_epoch
        && left.adapter_revision == right.adapter_revision
        && left.adapter_sha256 == right.adapter_sha256
        && left.model == right.model
        && left.configuration_sha256 == right.configuration_sha256
        && left.output_schema_sha256 == right.output_schema_sha256
        && left.capabilities_sha256 == right.capabilities_sha256
        && left.authorization_policy_version == right.authorization_policy_version
}
