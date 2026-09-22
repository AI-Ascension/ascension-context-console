// SPDX-License-Identifier: MIT

use super::render::{PreparedMaterial, digest, item_ref};
use super::types::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod debug_redaction;
#[cfg(test)]
mod debug_redaction_tests;
mod drafting;
mod lifecycle;
mod models;
mod records;
mod time;

use models::{DraftRecord, PreviewRecord};
pub(crate) use time::{format_time, parse_time};

/// Harness-facing control authority used by the synthetic console and by tests.  The target
/// adapter can inspect this state, but only this object changes pause, revision and plan epochs.
#[derive(Clone, Serialize, Deserialize)]
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

    pub fn command(&self, command_id: &str) -> Result<Receipt, ControlError> {
        self.commands
            .values()
            .find_map(|(_, receipt)| (receipt.command_id == command_id).then_some(receipt.clone()))
            .ok_or_else(|| ControlError::invalid("command_not_found", "command is unavailable"))
    }

    /// Look up a durable receipt by the caller-provided idempotency key.
    ///
    /// The owner-generated command ID is not necessarily known to a caller when a mutation was
    /// applied but its response was lost, so recovery must be keyed by the value the caller kept.
    pub fn receipt_for_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Result<Receipt, ControlError> {
        self.commands
            .get(idempotency_key)
            .map(|(_, receipt)| receipt.clone())
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

    /// Changes the host action catalog fingerprint while retaining the visible state label. The
    /// next approval must observe the new catalog bytes instead of silently reusing an old plan.
    pub fn advance_catalog(&mut self) {
        self.boundary.catalog_sha256 =
            digest(format!("catalog-{}", self.boundary.gate_epoch.saturating_add(1)).as_bytes());
        self.state.boundary = Some(self.boundary.clone());
        if self.state.pause_latched {
            self.state.status = "paused_stale".to_owned();
        }
        self.invalidate_all_previews("catalog_changed");
    }

    /// Changes the adapter/model/configuration fingerprints without changing host state. This is
    /// the deterministic fixture seam for an upgraded binary or configuration between preview and
    /// use; every existing preview is fenced at the new boundary.
    pub fn advance_provider_fingerprint(&mut self) {
        let next = self.boundary.gate_epoch.saturating_add(1);
        self.boundary.adapter_revision = format!("fixture-adapter-v{next}");
        self.boundary.adapter_sha256 = digest(self.boundary.adapter_revision.as_bytes());
        self.boundary.model = format!("synthetic-fixture-v{next}");
        self.boundary.configuration_sha256 = digest(format!("configuration-{next}").as_bytes());
        self.state.boundary = Some(self.boundary.clone());
        if self.state.pause_latched {
            self.state.status = "paused_stale".to_owned();
        }
        self.invalidate_all_previews("provider_fingerprint_changed");
    }

    /// Advances the deterministic fixture clock so expiry and command-window behavior can be
    /// exercised without depending on wall-clock sleeps.
    pub fn advance_time(&mut self, seconds: u64) {
        self.now = self.now.saturating_add(seconds);
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
                self.retained_item(&item)?;
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
                self.retained_item(&item)?;
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
                if draft.objective_item != revision.objective_item && !objective_authorized {
                    return Err(ControlError::forbidden(
                        "objective_authorization_required",
                        "restoring an objective requires the objective scope",
                    ));
                }
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
        if self
            .items
            .values()
            .any(|record| record.item.item_id == note_id && record.protected)
        {
            return Err(ControlError::forbidden(
                "protected_item",
                "note identity is reserved for host-owned content",
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
        if current.is_none() && draft.note_items.len() >= MAX_NOTES {
            return Err(ControlError::invalid("note_limit", "note limit is reached"));
        }
        let replaced_bytes = current
            .and_then(|version| self.items.get(&(note_id.to_owned(), version)))
            .map_or(0, |record| record.content.len());
        let total_note_bytes = draft
            .note_items
            .iter()
            .filter_map(|item| self.items.get(&(item.item_id.clone(), item.version)))
            .map(|record| record.content.len())
            .sum::<usize>();
        if total_note_bytes
            .saturating_sub(replaced_bytes)
            .saturating_add(text.len())
            > MAX_TOTAL_NOTE_BYTES
        {
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
}
