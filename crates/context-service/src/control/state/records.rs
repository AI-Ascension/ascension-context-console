// SPDX-License-Identifier: MIT

use super::super::types::*;
use super::ControlPlane;

impl ControlPlane {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Disables new management writes while retaining the active revision, pause latch, journal,
    /// and read projections. This is the safe fixture rollback/deactivation operation.
    pub fn deactivate(&mut self) {
        self.enabled = false;
        self.record_event(
            "management.disabled",
            None,
            None,
            Some(&self.state.active_revision_id.clone()),
            Some("safe_deactivation"),
        );
    }

    /// Re-enables the management fixture without changing its revision or scheduler state.
    pub fn activate(&mut self) {
        self.enabled = true;
        self.record_event(
            "management.enabled",
            None,
            None,
            Some(&self.state.active_revision_id.clone()),
            Some("explicit_activation"),
        );
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
            durable_control_store: "unverified".to_owned(),
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
                content_available: !record.content.is_empty()
                    && !record.protected
                    && record.expires_at > self.now
                    && std::str::from_utf8(&record.content).is_ok(),
                bytes: record.content.len(),
                expires_at: record.expires_text.clone(),
                locked_reason: record.locked_reason.clone(),
                content: (!record.protected
                    && record.expires_at > self.now
                    && std::str::from_utf8(&record.content).is_ok())
                .then(|| std::str::from_utf8(&record.content).ok().map(str::to_owned))
                .flatten(),
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

    /// Returns one immutable revision without materializing the complete revision history.
    pub fn get_revision(&self, revision_id: &str) -> Result<Revision, ControlError> {
        self.revisions
            .get(revision_id)
            .cloned()
            .ok_or_else(|| ControlError::invalid("revision_not_found", "revision is unavailable"))
    }

    /// Returns immutable preview projections without exposing their prepared input material.
    pub fn previews(&self) -> Vec<Preview> {
        self.previews
            .values()
            .map(|record| record.preview.clone())
            .collect()
    }

    /// Returns immutable command receipts for reconnect/read-after-restart consumers.
    pub fn receipts(&self) -> Vec<Receipt> {
        self.commands
            .values()
            .map(|(_, receipt)| receipt.clone())
            .collect()
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
}
