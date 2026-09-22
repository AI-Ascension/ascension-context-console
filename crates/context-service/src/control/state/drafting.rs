// SPDX-License-Identifier: MIT

use super::super::render::render;
use super::super::types::*;
use super::ControlPlane;
use super::models::{DraftRecord, PreviewRecord};
use super::time::{format_time, parse_time};

impl ControlPlane {
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
        if parse_time(&record.draft.expires_at).is_none()
            || parse_time(&record.draft.expires_at).unwrap_or(0) <= self.now
        {
            return Err(ControlError::conflict("expired_draft", "draft has expired"));
        }
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
        if parse_time(&draft.expires_at).is_none()
            || parse_time(&draft.expires_at).unwrap_or(0) <= self.now
        {
            return Err(ControlError::conflict("expired_draft", "draft has expired"));
        }
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
        if applicable && !risk_ack {
            blockers.push("unknown_total_budget".to_owned());
        }
        let material = if blockers.is_empty() {
            render(&preview_id, &self.boundary, &draft, &self.items, self.now).ok()
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
            budget_status: "bounded_unknown_total".to_owned(),
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

    pub(super) fn editable_item(&self, item: &ItemRef) -> Result<&ItemRecord, ControlError> {
        let record = self.retained_item(item)?;
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

    pub(super) fn retained_item(&self, item: &ItemRef) -> Result<&ItemRecord, ControlError> {
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
        Ok(record)
    }

    pub(super) fn invalidate_previews(&mut self, draft_id: &str, reason: &str) {
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
    pub(super) fn invalidate_all_previews(&mut self, reason: &str) {
        for record in self.previews.values_mut().filter(|record| !record.consumed) {
            record.preview.applicable = false;
            record.preview.blockers = vec![reason.to_owned()];
            record.preview.prepared_manifest_sha256 = None;
            record.material = None;
        }
    }
}
