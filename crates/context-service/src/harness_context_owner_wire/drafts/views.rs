use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::super::boundary::{ContextBoundary, ContextOwnerBinding};
use super::super::validation::{
    MAX_HARNESS_CONTEXT_BYTES, MAX_HARNESS_DRAFT_OPERATIONS, MAX_HARNESS_ITEMS,
    MAX_HARNESS_PAGE_SIZE, MAX_HARNESS_PUBLICATIONS, OwnerWireError, validate_digest,
    validate_identifier, validate_schema,
};
use super::{
    CONTEXT_OWNER_DRAFT_SCHEMA_V1, CONTEXT_OWNER_ITEMS_SCHEMA_V1, CONTEXT_OWNER_PREVIEW_SCHEMA_V1,
    CONTEXT_OWNER_REVISION_SCHEMA_V1, ContextDraft, ContextItemRef,
};
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerItemView {
    pub reference: ContextItemRef,
    pub kind: String,
    pub byte_length: u64,
    pub protected: bool,
    pub expires_at: u64,
    pub source_id: String,
    pub source_version: u64,
    pub source_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerItemsView {
    pub schema_version: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub binding_id: String,
    pub binding_digest: String,
    pub boundary: ContextBoundary,
    pub items: Vec<HarnessContextOwnerItemView>,
}

impl HarnessContextOwnerItemsView {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_ITEMS_SCHEMA_V1)?;
        validate_identifier("items_owner_id", &self.owner_id)?;
        validate_identifier("items_workflow_run_id", &self.workflow_run_id)?;
        validate_identifier("items_binding_id", &self.binding_id)?;
        validate_digest("items_binding_digest", &self.binding_digest)?;
        self.boundary.validate()?;
        if self.items.len() > MAX_HARNESS_ITEMS || self.workflow_run_id != self.boundary.run_id {
            return Err(OwnerWireError::OutOfBounds("items_view"));
        }
        let mut seen = BTreeSet::new();
        let mut total = 0_usize;
        for item in &self.items {
            item.reference.validate()?;
            validate_identifier("item_kind", &item.kind)?;
            validate_identifier("item_source_id", &item.source_id)?;
            validate_digest("item_source_digest", &item.source_digest)?;
            if item.source_version == 0
                || item.byte_length == 0
                || item.byte_length > MAX_HARNESS_CONTEXT_BYTES as u64
                || item.content.as_ref().is_some_and(|content| {
                    content.len() as u64 != item.byte_length
                        || content.len() > MAX_HARNESS_CONTEXT_BYTES
                })
                || !seen.insert((&item.reference.item_id, item.reference.version))
            {
                return Err(OwnerWireError::InvalidValue("items_view_entry"));
            }
            total = total
                .checked_add(item.content.as_ref().map_or(0, Vec::len))
                .ok_or(OwnerWireError::OutOfBounds("items_content"))?;
        }
        if total > MAX_HARNESS_CONTEXT_BYTES {
            return Err(OwnerWireError::OutOfBounds("items_content"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftEnvelope {
    pub schema_version: String,
    pub actor_subject: String,
    pub binding: ContextOwnerBinding,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_expires_at: Option<u64>,
    pub draft: ContextDraft,
}

impl HarnessContextOwnerDraftEnvelope {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_DRAFT_SCHEMA_V1)?;
        validate_identifier("draft_actor_subject", &self.actor_subject)?;
        self.binding.validate()?;
        self.draft.validate()?;
        if self.created_at == 0
            || self.updated_at < self.created_at
            || self
                .retention_expires_at
                .is_some_and(|expires| expires <= self.created_at || expires == u64::MAX)
            || self.draft.base_revision_id != self.binding.approved_revision_id
        {
            return Err(OwnerWireError::InvalidValue("draft_envelope"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerRevisionEnvelope {
    pub schema_version: String,
    pub revision_id: String,
    pub actor_subject: String,
    pub binding: ContextOwnerBinding,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_expires_at: Option<u64>,
    pub draft: ContextDraft,
}

impl HarnessContextOwnerRevisionEnvelope {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_REVISION_SCHEMA_V1)?;
        validate_identifier("revision_id", &self.revision_id)?;
        validate_identifier("revision_actor_subject", &self.actor_subject)?;
        self.binding.validate()?;
        self.draft.validate()?;
        if self.created_at == 0
            || self
                .retention_expires_at
                .is_some_and(|expires| expires <= self.created_at || expires == u64::MAX)
        {
            return Err(OwnerWireError::InvalidValue("revision_envelope"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerPreviewEnvelope {
    pub schema_version: String,
    pub preview_id: String,
    pub actor_subject: String,
    pub binding: ContextOwnerBinding,
    pub provider_config_digest: String,
    pub draft_id: String,
    pub draft_version: u64,
    pub base_revision_id: String,
    pub manifest_digest: String,
    pub effect_class: String,
    pub blockers: Vec<String>,
    pub created_at: u64,
    pub expires_at: u64,
}

impl HarnessContextOwnerPreviewEnvelope {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_PREVIEW_SCHEMA_V1)?;
        for (field, value) in [
            ("preview_id", self.preview_id.as_str()),
            ("preview_actor_subject", self.actor_subject.as_str()),
            ("preview_draft_id", self.draft_id.as_str()),
            ("preview_base_revision_id", self.base_revision_id.as_str()),
            ("preview_effect_class", self.effect_class.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        self.binding.validate()?;
        validate_digest(
            "preview_provider_config_digest",
            &self.provider_config_digest,
        )?;
        validate_digest("preview_manifest_digest", &self.manifest_digest)?;
        if self.draft_version == 0
            || self.created_at == 0
            || self.expires_at <= self.created_at
            || self.expires_at == u64::MAX
            || self.blockers.len() > MAX_HARNESS_DRAFT_OPERATIONS
        {
            return Err(OwnerWireError::InvalidValue("preview_envelope"));
        }
        for blocker in &self.blockers {
            validate_identifier("preview_blocker", blocker)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftListView {
    pub schema_version: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub drafts: Vec<HarnessContextOwnerDraftEnvelope>,
}

impl HarnessContextOwnerDraftListView {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            "ascension.harness.context-owner-drafts.v1",
        )?;
        validate_identifier("draft_list_owner_id", &self.owner_id)?;
        validate_identifier("draft_list_run_id", &self.workflow_run_id)?;
        if self.drafts.len() > MAX_HARNESS_PUBLICATIONS {
            return Err(OwnerWireError::OutOfBounds("draft_list"));
        }
        let mut ids = BTreeSet::new();
        for draft in &self.drafts {
            draft.validate()?;
            if draft.binding.owner_id != self.owner_id
                || draft.binding.workflow_run_id != self.workflow_run_id
                || !ids.insert(&draft.draft.draft_id)
            {
                return Err(OwnerWireError::CorrelationMismatch("draft_list"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerRevisionPage {
    pub schema_version: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub revisions: Vec<HarnessContextOwnerRevisionEnvelope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_after_revision_id: Option<String>,
}

impl HarnessContextOwnerRevisionPage {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            "ascension.harness.context-owner-revisions.v1",
        )?;
        validate_identifier("revision_page_owner_id", &self.owner_id)?;
        validate_identifier("revision_page_run_id", &self.workflow_run_id)?;
        if self.revisions.len() > MAX_HARNESS_PAGE_SIZE as usize {
            return Err(OwnerWireError::OutOfBounds("revision_page"));
        }
        if let Some(cursor) = &self.next_after_revision_id {
            validate_identifier("next_after_revision_id", cursor)?;
        }
        for revision in &self.revisions {
            revision.validate()?;
            if revision.binding.owner_id != self.owner_id
                || revision.binding.workflow_run_id != self.workflow_run_id
            {
                return Err(OwnerWireError::CorrelationMismatch("revision_page"));
            }
        }
        Ok(())
    }
}
