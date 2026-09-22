// SPDX-License-Identifier: MIT

use super::super::render::digest;
use super::super::types::*;
use super::ControlPlane;
use super::time::{format_time, parse_time, same_external_boundary};
use serde::Deserialize;
use std::collections::BTreeSet;

impl ControlPlane {
    pub fn recover_journal(bytes: &[u8]) -> Result<Self, ControlError> {
        Self::recover_journal_with_epoch(bytes, true)
    }

    pub(crate) fn recover_journal_without_epoch(bytes: &[u8]) -> Result<Self, ControlError> {
        Self::recover_journal_with_epoch(bytes, false)
    }

    pub(super) fn recover_journal_with_epoch(
        bytes: &[u8],
        increment_controller_epoch: bool,
    ) -> Result<Self, ControlError> {
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
        let mut seen_items = BTreeSet::new();
        for (key, item) in journal.items {
            if key.0 != item.item.item_id
                || key.1 != item.item.version
                || !item.item.valid()
                || item.scope != journal.plane.scope
                || item.content.len() > MAX_COMPONENT_BYTES
                || item.expires_at == 0
                || !seen_items.insert(key.clone())
            {
                return Err(ControlError::invalid(
                    "journal_invalid",
                    "control journal item integrity is invalid",
                ));
            }
            journal.plane.items.insert(key, item);
        }
        journal.plane.validate_journal_integrity()?;
        if increment_controller_epoch {
            journal.plane.state.controller_epoch =
                journal.plane.state.controller_epoch.saturating_add(1);
            journal.plane.boundary.controller_epoch = journal.plane.state.controller_epoch;
            journal.plane.sync_boundary();
        }
        Ok(journal.plane)
    }

    pub(super) fn validate_journal_integrity(&self) -> Result<(), ControlError> {
        if self.state.scope != self.scope
            || self.boundary.scope != self.scope
            || self.state.active_revision_id.is_empty()
            || !self.revisions.contains_key(&self.state.active_revision_id)
        {
            return Err(ControlError::invalid(
                "journal_invalid",
                "control journal scope or active revision is invalid",
            ));
        }
        for (key, record) in &self.items {
            if key.0 != record.item.item_id
                || key.1 != record.item.version
                || record.scope != self.scope
                || !record.item.valid()
                || digest(&record.content) != record.item.sha256
                || record.content.len() > MAX_COMPONENT_BYTES
                || record.expires_at == 0
                || parse_time(&record.expires_text) != Some(record.expires_at)
            {
                return Err(ControlError::invalid(
                    "journal_invalid",
                    "control journal retained item is invalid",
                ));
            }
        }
        for record in self.drafts.values() {
            self.validate_references(
                &record.draft.selected_items,
                &record.draft.note_items,
                record.draft.objective_item.as_ref(),
                &record.draft.pinned_item_ids,
            )?;
        }
        for revision in self.revisions.values() {
            self.validate_references(
                &revision.selected_items,
                &revision.note_items,
                revision.objective_item.as_ref(),
                &revision.pinned_item_ids,
            )?;
        }
        for record in self.previews.values() {
            if record.preview.scope != self.scope
                || record
                    .preview
                    .selected_items
                    .iter()
                    .any(|reference| !self.retained_reference(reference))
                || (record.preview.applicable && record.material.is_none())
                || record.material.as_ref().is_some_and(|material| {
                    material.manifest_sha256
                        != record
                            .preview
                            .prepared_manifest_sha256
                            .clone()
                            .unwrap_or_default()
                })
            {
                return Err(ControlError::invalid(
                    "journal_invalid",
                    "control journal preview references missing material",
                ));
            }
        }
        if self
            .continuation_preview_id
            .as_ref()
            .is_some_and(|preview_id| !self.previews.contains_key(preview_id))
            || self.prepared_input.as_ref().is_some_and(|input| {
                !self.previews.values().any(|record| {
                    record.consumed
                        && record
                            .material
                            .as_ref()
                            .is_some_and(|material| material.input == *input)
                })
            })
        {
            return Err(ControlError::invalid(
                "journal_invalid",
                "control journal continuation is invalid",
            ));
        }
        if self.events.iter().any(|event| event.scope != self.scope)
            || self
                .relations
                .iter()
                .any(|relation| relation.scope != self.scope)
            || self
                .commands
                .values()
                .any(|(_, receipt)| receipt.scope != self.scope)
        {
            return Err(ControlError::invalid(
                "journal_invalid",
                "control journal record scope is invalid",
            ));
        }
        Ok(())
    }

    pub(super) fn validate_references(
        &self,
        selected: &[ItemRef],
        notes: &[ItemRef],
        objective: Option<&ItemRef>,
        pinned: &[String],
    ) -> Result<(), ControlError> {
        if selected
            .iter()
            .any(|reference| !self.retained_reference(reference))
            || notes
                .iter()
                .any(|reference| !self.retained_reference(reference))
            || objective.is_some_and(|reference| !self.retained_reference(reference))
            || pinned.iter().any(|item_id| {
                !selected
                    .iter()
                    .any(|reference| reference.item_id == *item_id)
            })
        {
            return Err(ControlError::invalid(
                "journal_invalid",
                "control journal references missing item bytes",
            ));
        }
        Ok(())
    }

    pub(super) fn retained_reference(&self, reference: &ItemRef) -> bool {
        reference.valid()
            && self
                .items
                .get(&(reference.item_id.clone(), reference.version))
                .is_some_and(|record| {
                    record.scope == self.scope
                        && record.item == *reference
                        && digest(&record.content) == reference.sha256
                })
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
            || parse_time(&preview.preview.expires_at).is_none()
            || parse_time(&preview.preview.expires_at).unwrap_or(0) <= self.now
            || preview.preview.boundary != self.boundary
        {
            return Err(ControlError::conflict(
                "preview_stale",
                "preview approval is stale",
            ));
        }
        let draft = self.get_draft(&preview.preview.draft_id)?;
        let current_revision = self
            .revisions
            .get(&self.state.active_revision_id)
            .ok_or_else(|| {
                ControlError::invalid("revision_not_found", "active revision is unavailable")
            })?;
        if draft.selected_items == current_revision.selected_items
            && draft.pinned_item_ids == current_revision.pinned_item_ids
            && draft.note_items == current_revision.note_items
            && draft.objective_item == current_revision.objective_item
        {
            if let Some(entry) = self.previews.get_mut(preview_id) {
                entry.consumed = true;
            }
            self.continuation_preview_id = Some(preview_id.to_owned());
            self.prepared_input = None;
            let receipt = self.receipt(&command, "no_change", None);
            self.save_command(&command, &receipt);
            self.record_event(
                "revision.no_change",
                Some(&receipt.command_id),
                Some(preview_id),
                Some(&self.state.active_revision_id.clone()),
                Some("configuration_identical"),
            );
            return Ok(receipt);
        }
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
        match (
            self.continuation_preview_id.as_deref(),
            command.expected_preview_id.as_deref(),
        ) {
            (Some(_), None) => {
                return Err(ControlError::invalid(
                    "preview_required",
                    "resume must name the approved continuation",
                ));
            }
            (Some(current), Some(expected)) => {
                if current != expected {
                    return Err(ControlError::conflict(
                        "preview_stale",
                        "approved continuation is unavailable",
                    ));
                }
            }
            (None, Some(_)) => {
                return Err(ControlError::conflict(
                    "preview_stale",
                    "approved continuation is unavailable",
                ));
            }
            (None, None) => {}
        }
        if let Some(preview_id) = self.continuation_preview_id.as_deref() {
            let continuation = self.previews.get(preview_id).ok_or_else(|| {
                ControlError::conflict("preview_stale", "approved continuation is unavailable")
            })?;
            if parse_time(&continuation.preview.expires_at).is_none()
                || parse_time(&continuation.preview.expires_at).unwrap_or(0) <= self.now
            {
                return Err(ControlError::conflict(
                    "preview_stale",
                    "approved continuation has expired",
                ));
            }
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
        self.prepared_input = None;
        Ok(receipt)
    }

    pub(super) fn command_guard(
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

    pub(super) fn idempotent_result(
        &self,
        command: &Command,
    ) -> Result<Option<Receipt>, ControlError> {
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

    pub(super) fn receipt(
        &mut self,
        command: &Command,
        effect: &str,
        reason_code: Option<&str>,
    ) -> Receipt {
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

    pub(super) fn save_command(&mut self, command: &Command, receipt: &Receipt) {
        let digest = serde_json::to_vec(command)
            .map(|bytes| digest(&bytes))
            .unwrap_or_default();
        self.commands
            .insert(command.idempotency_key.clone(), (digest, receipt.clone()));
    }

    pub(super) fn check_scope(&self, scope: &Scope) -> Result<(), ControlError> {
        scope.same(&self.scope).then_some(()).ok_or_else(|| {
            ControlError::forbidden("scope_mismatch", "request scope is outside this run")
        })
    }
    pub(super) fn check_active_revision(&self, revision: &str) -> Result<(), ControlError> {
        (revision == self.state.active_revision_id)
            .then_some(())
            .ok_or_else(|| {
                ControlError::conflict("stale_revision", "active revision does not match")
            })
    }
    pub(super) fn require_enabled(&self) -> Result<(), ControlError> {
        self.enabled.then_some(()).ok_or_else(|| {
            ControlError::forbidden("management_disabled", "context management is disabled")
        })
    }
    pub(super) fn quiescent(&self) -> bool {
        self.state.outstanding_provider_attempts.is_empty()
            && self.state.unresolved_operations.is_empty()
    }
    pub(super) fn sync_boundary(&mut self) {
        self.boundary.control_version = self.state.control_version;
        self.boundary.controller_epoch = self.state.controller_epoch;
        self.boundary.gate_epoch = self.state.gate_epoch;
        self.state.boundary = Some(self.boundary.clone());
    }
    pub(super) fn record_event(
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
    pub(super) fn id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}
