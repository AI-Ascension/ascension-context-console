use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::boundary::{ContextBindingSource, ContextBoundary, ContextOwnerBinding};
use super::digest::sha256_hex;
use super::validation::{
    MAX_HARNESS_PUBLICATIONS, OwnerWireError, validate_digest, validate_identifier, validate_schema,
};

pub const CONTEXT_OWNER_PUBLICATION_REQUEST_SCHEMA_V1: &str =
    "ascension.harness.context-owner-draft-publication-request.v1";
pub const CONTEXT_OWNER_PUBLICATION_RECEIPT_SCHEMA_V1: &str =
    "ascension.harness.context-owner-draft-publication-receipt.v1";
pub const CONTEXT_OWNER_PUBLICATION_LOOKUP_SCHEMA_V1: &str =
    "ascension.harness.context-owner-draft-publication-lookup.v1";
pub const CONTEXT_OWNER_PUBLISHED_SOURCES_SCHEMA_V1: &str =
    "ascension.harness.context-owner-published-sources.v1";

const PUBLICATION_DIGEST_DOMAIN: &[u8] =
    b"ascension.context-control.draft-publication.request-digest.v1\0";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftPublicationRequest {
    pub schema_version: String,
    pub request_id: String,
    pub draft_id: String,
    pub expected_draft_version: u64,
    pub expected_owner_state_version: u64,
    pub expected_base_revision_id: String,
    pub expected_binding_id: String,
    pub expected_binding_digest: String,
    pub expected_boundary: ContextBoundary,
}

impl HarnessContextOwnerDraftPublicationRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_PUBLICATION_REQUEST_SCHEMA_V1,
        )?;
        validate_identifier("publication_request_id", &self.request_id)?;
        validate_identifier("publication_draft_id", &self.draft_id)?;
        validate_identifier(
            "publication_base_revision_id",
            &self.expected_base_revision_id,
        )?;
        validate_identifier("publication_binding_id", &self.expected_binding_id)?;
        validate_digest("publication_binding_digest", &self.expected_binding_digest)?;
        self.expected_boundary.validate()?;
        if self.expected_draft_version == 0 || self.expected_owner_state_version == 0 {
            return Err(OwnerWireError::InvalidValue("publication_versions"));
        }
        Ok(())
    }

    /// Computes the exact H391 v1 binary frame digest, not a JSON hash. Digest bytes are raw
    /// 32-byte values; strings are unsigned-64-bit-big-endian length-prefixed UTF-8.
    pub fn request_digest(&self) -> Result<String, OwnerWireError> {
        self.validate()?;
        let boundary = &self.expected_boundary;
        let mut frame = PUBLICATION_DIGEST_DOMAIN.to_vec();
        append_lp(&mut frame, &self.schema_version)?;
        append_lp(&mut frame, &self.request_id)?;
        append_lp(&mut frame, &self.draft_id)?;
        append_u64(&mut frame, self.expected_draft_version);
        append_u64(&mut frame, self.expected_owner_state_version);
        append_lp(&mut frame, &self.expected_base_revision_id)?;
        append_lp(&mut frame, &self.expected_binding_id)?;
        append_digest(&mut frame, &self.expected_binding_digest)?;
        append_lp(&mut frame, &boundary.run_id)?;
        append_lp(&mut frame, &boundary.episode_id)?;
        append_lp(&mut frame, &boundary.agent_id)?;
        append_lp(&mut frame, &boundary.state_id)?;
        append_u64(&mut frame, boundary.generation);
        append_digest(&mut frame, &boundary.observation_sha256)?;
        append_digest(&mut frame, &boundary.catalog_sha256)?;
        append_lp(&mut frame, &boundary.adapter_revision)?;
        append_lp(&mut frame, &boundary.model_revision)?;
        append_digest(&mut frame, &boundary.configuration_sha256)?;
        append_digest(&mut frame, &boundary.output_schema_sha256)?;
        append_u64(&mut frame, boundary.controller_epoch);
        append_u64(&mut frame, boundary.gate_epoch);
        append_u64(&mut frame, boundary.control_version);
        Ok(sha256_hex(&frame))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftPublicationReceipt {
    pub schema_version: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub actor_subject: String,
    pub binding: ContextOwnerBinding,
    pub boundary: ContextBoundary,
    pub request_id: String,
    pub request_digest: String,
    pub draft_id: String,
    pub draft_version: u64,
    pub base_revision_id: String,
    pub expected_owner_state_version: u64,
    pub resulting_owner_state_version: u64,
    pub source_id: String,
    pub source_version: u64,
    pub source_digest: String,
    pub published_at: u64,
    pub expires_at: u64,
}

impl HarnessContextOwnerDraftPublicationReceipt {
    pub fn validate_for(
        &self,
        request: &HarnessContextOwnerDraftPublicationRequest,
        expected_actor_subject: &str,
        expected_workflow_run_id: &str,
    ) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_PUBLICATION_RECEIPT_SCHEMA_V1,
        )?;
        request.validate()?;
        self.binding.validate()?;
        for (field, value) in [
            ("publication_owner_id", self.owner_id.as_str()),
            ("publication_run_id", self.workflow_run_id.as_str()),
            ("publication_actor_subject", self.actor_subject.as_str()),
            ("publication_request_id", self.request_id.as_str()),
            ("publication_draft_id", self.draft_id.as_str()),
            (
                "publication_base_revision_id",
                self.base_revision_id.as_str(),
            ),
            ("publication_source_id", self.source_id.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("publication_request_digest", &self.request_digest)?;
        validate_digest("publication_source_digest", &self.source_digest)?;
        let resulting_version = request.expected_owner_state_version.checked_add(1).ok_or(
            OwnerWireError::OutOfBounds("publication_owner_state_version"),
        )?;
        if self.actor_subject != expected_actor_subject
            || self.workflow_run_id != expected_workflow_run_id
            || self.workflow_run_id != request.expected_boundary.run_id
            || self.request_id != request.request_id
            || self.request_digest != request.request_digest()?
            || self.draft_id != request.draft_id
            || self.draft_version != request.expected_draft_version
            || self.base_revision_id != request.expected_base_revision_id
            || self.expected_owner_state_version != request.expected_owner_state_version
            || self.resulting_owner_state_version != resulting_version
            || self.source_version != 1
            || self.boundary != request.expected_boundary
            || self.binding.owner_id != self.owner_id
            || self.binding.workflow_run_id != self.workflow_run_id
            || self.binding.binding_id != request.expected_binding_id
            || self.binding.binding_digest != request.expected_binding_digest
            || self.binding.boundary != self.boundary
            || self.published_at == 0
            || self.expires_at <= self.published_at
            || self.expires_at == u64::MAX
        {
            return Err(OwnerWireError::CorrelationMismatch("publication_receipt"));
        }
        Ok(())
    }
}

/// Metadata-only published-source projection. Receipt bodies, actor subjects, and request IDs are
/// intentionally absent from this route's result type.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerPublishedSourcesView {
    pub schema_version: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub definition_digest: String,
    pub instance_id: String,
    pub binding: ContextOwnerBinding,
    pub boundary: ContextBoundary,
    pub owner_state_version: u64,
    pub active_source: Option<ContextBindingSource>,
    pub publications: Vec<ContextBindingSource>,
}

impl HarnessContextOwnerPublishedSourcesView {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_PUBLISHED_SOURCES_SCHEMA_V1,
        )?;
        validate_identifier("published_owner_id", &self.owner_id)?;
        validate_identifier("published_workflow_run_id", &self.workflow_run_id)?;
        validate_identifier("published_instance_id", &self.instance_id)?;
        validate_digest("published_definition_digest", &self.definition_digest)?;
        self.binding.validate()?;
        self.boundary.validate()?;
        if self.owner_state_version == 0
            || self.binding.owner_id != self.owner_id
            || self.binding.workflow_run_id != self.workflow_run_id
            || self.binding.instance_id != self.instance_id
            || self.binding.definition_digest != self.definition_digest
            || self.binding.boundary != self.boundary
            || self.publications.len() > MAX_HARNESS_PUBLICATIONS
        {
            return Err(OwnerWireError::CorrelationMismatch(
                "published_sources_view",
            ));
        }
        let mut identities = BTreeSet::new();
        for source in &self.publications {
            source.validate()?;
            if !identities.insert((&source.source_id, source.version)) {
                return Err(OwnerWireError::InvalidValue("duplicate_publication"));
            }
        }
        if let Some(active) = &self.active_source {
            active.validate()?;
            if !identities.contains(&(&active.source_id, active.version)) {
                return Err(OwnerWireError::CorrelationMismatch("active_source"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftPublicationLookupRequest {
    pub schema_version: String,
    pub request: HarnessContextOwnerDraftPublicationRequest,
}

impl HarnessContextOwnerDraftPublicationLookupRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_PUBLICATION_LOOKUP_SCHEMA_V1,
        )?;
        self.request.validate()
    }
}

fn append_lp(frame: &mut Vec<u8>, value: &str) -> Result<(), OwnerWireError> {
    let length = u64::try_from(value.len())
        .map_err(|_| OwnerWireError::OutOfBounds("publication_length_prefixed_field"))?;
    append_u64(frame, length);
    frame.extend_from_slice(value.as_bytes());
    Ok(())
}

fn append_u64(frame: &mut Vec<u8>, value: u64) {
    frame.extend_from_slice(&value.to_be_bytes());
}

fn append_digest(frame: &mut Vec<u8>, value: &str) -> Result<(), OwnerWireError> {
    validate_digest("publication_digest_field", value)?;
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| OwnerWireError::InvalidDigest("publication_digest_field"))?;
    }
    frame.extend_from_slice(&bytes);
    Ok(())
}
