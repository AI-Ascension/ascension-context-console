use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::boundary::{
    ContextBindingSource, ContextBoundary, ContextEffectiveLimits, ContextOwnerBinding,
};
use super::digest::sha256_hex;
use super::drafts::{ContextDraft, ContextItem};
use super::validation::{
    MAX_HARNESS_CONTEXT_BYTES, MAX_HARNESS_ITEMS, OwnerWireError, validate_digest,
    validate_identifier, validate_schema,
};

pub const CONTEXT_SOURCE_UPLOAD_SCHEMA_V1: &str =
    "ascension.context-owner.context-source-upload.v1";
pub const CONTEXT_SOURCE_ADOPTION_SCHEMA_V1: &str =
    "ascension.context-owner.context-source-adoption.v1";
pub const CONTEXT_OWNER_SOURCE_STATUS_SCHEMA_V1: &str = "ascension.context-owner.source-status.v1";

/// Exact Harness `ContextSourceDocument` body, including sorted map keys and byte-array encoding.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceDocument {
    pub draft: ContextDraft,
    pub items: BTreeMap<String, ContextItem>,
}

impl ContextSourceDocument {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        self.draft.validate()?;
        if self.items.len() > MAX_HARNESS_ITEMS {
            return Err(OwnerWireError::OutOfBounds("source_items"));
        }
        let mut total = 0_usize;
        for (key, item) in &self.items {
            validate_identifier("source_item_key", key)?;
            item.validate()?;
            if key != &item.reference.item_id {
                return Err(OwnerWireError::CorrelationMismatch("source_item_key"));
            }
            total = total
                .checked_add(item.bytes.len())
                .ok_or(OwnerWireError::OutOfBounds("source_content"))?;
        }
        if total > MAX_HARNESS_CONTEXT_BYTES {
            return Err(OwnerWireError::OutOfBounds("source_content"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceUpload {
    pub schema_version: String,
    pub document: ContextSourceDocument,
}

impl ContextSourceUpload {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_SOURCE_UPLOAD_SCHEMA_V1)?;
        self.document.validate()
    }

    pub fn document_digest(&self) -> Result<String, OwnerWireError> {
        let bytes = super::validation::bounded_json_bytes(&self.document)?;
        Ok(sha256_hex(&bytes))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceAdoptionRequest {
    pub schema_version: String,
    pub idempotency_key: String,
    pub expected_control_version: u64,
    pub expected_revision_id: String,
    pub expected_boundary: ContextBoundary,
}

impl ContextSourceAdoptionRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_SOURCE_ADOPTION_SCHEMA_V1)?;
        validate_identifier("source_adoption_idempotency_key", &self.idempotency_key)?;
        validate_identifier("source_adoption_revision_id", &self.expected_revision_id)?;
        self.expected_boundary.validate()?;
        if self.expected_control_version == 0
            || self.expected_control_version != self.expected_boundary.control_version
        {
            return Err(OwnerWireError::InvalidValue(
                "source_adoption_control_version",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourcePublication {
    pub schema_version: String,
    pub source: ContextBindingSource,
}

impl ContextSourcePublication {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        // Harness intentionally uses the upload schema for the source-publication view.
        validate_schema(&self.schema_version, CONTEXT_SOURCE_UPLOAD_SCHEMA_V1)?;
        self.source.validate()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveContextSource {
    pub source_id: String,
    pub version: u64,
    pub digest: String,
    pub active_revision_id: String,
}

impl ActiveContextSource {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_identifier("active_source_id", &self.source_id)?;
        validate_identifier("active_revision_id", &self.active_revision_id)?;
        validate_digest("active_source_digest", &self.digest)?;
        if self.version == 0 {
            return Err(OwnerWireError::InvalidValue("active_source_version"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerSourceStatus {
    pub schema_version: String,
    pub owner_id: String,
    pub owner_version: String,
    pub workflow_run_id: String,
    pub definition_digest: String,
    pub instance_id: String,
    pub boundary: ContextBoundary,
    pub active_revision_id: String,
    pub active_source: Option<ActiveContextSource>,
}

impl ContextOwnerSourceStatus {
    pub fn validate(&self, expected_workflow_run_id: &str) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_SOURCE_STATUS_SCHEMA_V1)?;
        for (field, value) in [
            ("source_status_owner_id", self.owner_id.as_str()),
            ("source_status_owner_version", self.owner_version.as_str()),
            ("source_status_run_id", self.workflow_run_id.as_str()),
            ("source_status_instance_id", self.instance_id.as_str()),
            (
                "source_status_active_revision_id",
                self.active_revision_id.as_str(),
            ),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("source_status_definition_digest", &self.definition_digest)?;
        self.boundary.validate()?;
        if self.workflow_run_id != expected_workflow_run_id
            || self.workflow_run_id != self.boundary.run_id
        {
            return Err(OwnerWireError::CorrelationMismatch("source_status_run"));
        }
        if let Some(active_source) = &self.active_source {
            active_source.validate()?;
            if active_source.active_revision_id != self.active_revision_id {
                return Err(OwnerWireError::CorrelationMismatch(
                    "source_status_revision",
                ));
            }
        }
        Ok(())
    }
}

/// The association route returns the complete binding. This type is used for the separately
/// exposed effective-limits projection if an operator chooses to request it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerControlLimits {
    pub schema_version: String,
    pub owner_id: String,
    pub owner_version: String,
    pub catalog_digest: String,
    pub max_control_events: u64,
}

impl ContextOwnerControlLimits {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            "ascension.harness.context-owner-control-limits.v1",
        )?;
        validate_identifier("control_limits_owner_id", &self.owner_id)?;
        validate_identifier("control_limits_owner_version", &self.owner_version)?;
        validate_digest("control_limits_catalog_digest", &self.catalog_digest)?;
        if self.max_control_events == 0 || self.max_control_events > 4096 {
            return Err(OwnerWireError::OutOfBounds("max_control_events"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerRenderLimitsWitness {
    pub owner_id: String,
    pub owner_version: String,
    pub binding: ContextOwnerBinding,
    pub effective_limits: ContextEffectiveLimits,
}
