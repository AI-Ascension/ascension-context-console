// SPDX-License-Identifier: MIT

//! Versioned, bounded control-plane records for Phase 2 context editing.
//!
//! These records deliberately contain no game command or provider credential.  The harness owns
//! execution authority; the console only prepares and authorizes a future provider input.

use serde::{Deserialize, Serialize};

pub const DRAFT_SCHEMA: &str = "ascension.context-control.draft.v1";
pub const PATCH_SCHEMA: &str = "ascension.context-control.patch.v1";
pub const PREVIEW_SCHEMA: &str = "ascension.context-control.preview.v1";
pub const COMMAND_SCHEMA: &str = "ascension.context-control.command.v1";
pub const RECEIPT_SCHEMA: &str = "ascension.context-control.receipt.v1";
pub const STATE_SCHEMA: &str = "ascension.context-control.state.v1";
pub const REVISION_SCHEMA: &str = "ascension.context-control.revision.v1";
pub const EVENT_SCHEMA: &str = "ascension.context-control.event.v1";
pub const RELATION_SCHEMA: &str = "ascension.context-control.relation.v1";
pub const CAPABILITIES_SCHEMA: &str = "ascension.context-control.capabilities.v1";
pub const JOURNAL_SCHEMA: &str = "ascension.context-control.journal.v1";
pub const MEMORY_BINDING_SCHEMA: &str = "ascension.context-memory.binding.v1";

pub const MAX_ITEMS: usize = 64;
pub const MAX_NOTES: usize = 16;
pub const MAX_OPERATIONS: usize = 32;
pub const MAX_NOTE_BYTES: usize = 4096;
pub const MAX_OBJECTIVE_BYTES: usize = 512;
pub const MAX_TOTAL_NOTE_BYTES: usize = 32 * 1024;
pub const MAX_COMPONENT_BYTES: usize = 128 * 1024;
pub const MAX_EVENTS: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

impl Scope {
    pub fn new(
        project_id: impl Into<String>,
        run_id: impl Into<String>,
        episode_id: impl Into<String>,
        agent_id: impl Into<String>,
    ) -> Result<Self, ControlError> {
        let scope = Self {
            project_id: project_id.into(),
            run_id: run_id.into(),
            episode_id: episode_id.into(),
            agent_id: agent_id.into(),
        };
        if [
            &scope.project_id,
            &scope.run_id,
            &scope.episode_id,
            &scope.agent_id,
        ]
        .iter()
        .all(|value| valid_id(value))
        {
            Ok(scope)
        } else {
            Err(ControlError::invalid(
                "invalid_scope",
                "scope identifier is invalid",
            ))
        }
    }

    pub fn same(&self, other: &Self) -> bool {
        self == other
    }
}

/// Metadata supplied by the harness when a reviewed memory selection is adopted. The target
/// stores only immutable identities and digests; policy/source bytes remain harness-owned.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryBindingRecord {
    pub schema: String,
    pub binding_id: String,
    pub phase2_revision_id: String,
    pub phase2_preview_id: String,
    pub policy_id: String,
    pub policy_version: u64,
    pub selection_sha256: String,
    pub audit_sha256: String,
}

impl MemoryBindingRecord {
    pub fn validate(&self) -> Result<(), ControlError> {
        if self.schema != MEMORY_BINDING_SCHEMA
            || !valid_id(&self.binding_id)
            || !valid_id(&self.phase2_revision_id)
            || !valid_id(&self.phase2_preview_id)
            || !valid_id(&self.policy_id)
            || self.policy_version == 0
            || !valid_digest(&self.selection_sha256)
            || !valid_digest(&self.audit_sha256)
        {
            return Err(ControlError::invalid(
                "invalid_memory_binding",
                "memory binding metadata is invalid",
            ));
        }
        Ok(())
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ItemRef {
    pub item_id: String,
    pub version: u64,
    pub sha256: String,
}

impl ItemRef {
    pub fn valid(&self) -> bool {
        valid_id(&self.item_id)
            && self.version > 0
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            && self.sha256.bytes().all(|byte| !byte.is_ascii_uppercase())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EligibleItem {
    pub item: ItemRef,
    pub kind: String,
    pub protected: bool,
    pub scope: Scope,
    pub content_available: bool,
    pub bytes: usize,
    pub expires_at: String,
    pub locked_reason: Option<String>,
    pub content: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub scope: Scope,
    pub state_id: String,
    pub generation: u64,
    pub observation_sha256: String,
    pub catalog_sha256: String,
    pub controller_epoch: u64,
    pub gate_epoch: u64,
    pub control_version: u64,
    pub lease_epoch: u64,
    pub adapter_revision: String,
    pub adapter_sha256: String,
    pub model: String,
    pub configuration_sha256: String,
    pub output_schema_sha256: String,
    pub capabilities_sha256: String,
    pub authorization_policy_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub schema: String,
    pub scope: Scope,
    pub draft_id: String,
    pub version: u64,
    pub base_revision_id: String,
    pub selected_items: Vec<ItemRef>,
    pub pinned_item_ids: Vec<String>,
    pub note_items: Vec<ItemRef>,
    pub objective_item: Option<ItemRef>,
    pub author_ref: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Operation {
    #[serde(rename = "include_item")]
    IncludeItem { item: ItemRef },
    #[serde(rename = "exclude_item")]
    ExcludeItem { item: ItemRef },
    #[serde(rename = "pin_item")]
    PinItem { item: ItemRef },
    #[serde(rename = "unpin_item")]
    UnpinItem { item: ItemRef },
    #[serde(rename = "put_note")]
    PutNote {
        note_id: String,
        expected_note_version: Option<u64>,
        text: String,
        expires_at: String,
    },
    #[serde(rename = "remove_note")]
    RemoveNote {
        note_id: String,
        expected_note_version: u64,
    },
    #[serde(rename = "set_objective")]
    SetObjective { text: String },
    #[serde(rename = "restore_configuration")]
    RestoreConfiguration { source_revision_id: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub schema: String,
    pub scope: Scope,
    pub draft_id: String,
    pub expected_draft_version: u64,
    pub expected_active_revision_id: String,
    pub operations: Vec<Operation>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PreviewComponent {
    pub component_id: String,
    pub ordinal: u64,
    pub kind: String,
    pub sha256: String,
    pub bytes: usize,
    pub protected: bool,
    pub content_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub schema: String,
    pub preview_id: String,
    pub scope: Scope,
    pub draft_id: String,
    pub draft_version: u64,
    pub base_revision_id: String,
    pub boundary: Boundary,
    pub applicable: bool,
    pub blockers: Vec<String>,
    pub prepared_manifest_sha256: Option<String>,
    pub components: Vec<PreviewComponent>,
    pub selected_items: Vec<ItemRef>,
    pub budget_status: String,
    pub unknown_total_risk_acknowledged: bool,
    pub provider_added_context: String,
    pub model_execution_id: Option<String>,
    pub provider_attempt_id: Option<String>,
    pub expires_at: String,
    pub effect_class: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub schema: String,
    pub scope: Scope,
    pub idempotency_key: String,
    pub command_window_id: String,
    pub expected_control_version: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_active_revision_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_preview_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub command_id: String,
    pub scope: Scope,
    pub kind: String,
    pub status: String,
    pub effect: String,
    pub control_version: u64,
    pub active_revision_id: String,
    pub paused: bool,
    pub reason_code: Option<String>,
    pub observed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub schema: String,
    pub scope: Scope,
    pub status: String,
    pub control_version: u64,
    pub controller_epoch: u64,
    pub gate_epoch: u64,
    pub plan_epoch: u64,
    pub active_revision_id: String,
    pub pause_latched: bool,
    pub stop_latched: bool,
    pub outstanding_provider_attempts: Vec<String>,
    pub unresolved_operations: Vec<String>,
    pub boundary: Option<Boundary>,
    pub command_window_id: String,
    pub command_window_expires_at: String,
    pub last_sequence: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub schema: String,
    pub scope: Scope,
    pub revision_id: String,
    pub sequence: u64,
    pub parent_revision_id: String,
    pub selected_items: Vec<ItemRef>,
    pub pinned_item_ids: Vec<String>,
    pub note_items: Vec<ItemRef>,
    pub objective_item: Option<ItemRef>,
    pub approved_preview_id: String,
    pub approved_manifest_sha256: String,
    pub author_ref: String,
    pub intervention_id: String,
    pub committed_at: String,
    pub plan_epoch: u64,
    pub state_after_commit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub schema: String,
    pub event_id: String,
    pub scope: Scope,
    pub sequence: u64,
    pub event_type: String,
    pub command_id: Option<String>,
    pub revision_id: Option<String>,
    pub preview_id: Option<String>,
    pub phase1_snapshot_id: Option<String>,
    pub model_execution_id: Option<String>,
    pub provider_attempt_id: Option<String>,
    pub action_id: Option<String>,
    pub reason_code: Option<String>,
    pub observed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    pub schema: String,
    pub scope: Scope,
    pub relation_id: String,
    pub revision_id: String,
    pub preview_id: Option<String>,
    pub phase1_snapshot_id: String,
    pub model_execution_id: String,
    pub provider_attempt_id: String,
    pub plan_id: Option<String>,
    pub action_id: Option<String>,
    pub intervention_id: Option<String>,
    pub first_approved_input: bool,
    pub evidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub schema: String,
    pub product_phase: u8,
    pub scope: Scope,
    pub enabled: bool,
    pub adapter_revision: String,
    pub supported_operations: Vec<String>,
    pub exact_application_preview: String,
    pub optional_images: String,
    pub durable_control_store: String,
    pub provider_added_context: String,
    pub context_compact: bool,
    pub persistent_provider_sessions: bool,
    pub direct_game_dispatch: bool,
    pub commit_auto_resumes: bool,
}

/// Private registry entry used by the renderer.  Its text never appears in control errors or
/// telemetry; callers receive it only through an authenticated eligible-item projection.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ItemRecord {
    pub item: ItemRef,
    pub kind: String,
    pub protected: bool,
    pub scope: Scope,
    pub content: Vec<u8>,
    pub expires_at: u64,
    pub expires_text: String,
    pub locked_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

impl ControlError {
    pub fn invalid(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn forbidden(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ControlError {}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}
