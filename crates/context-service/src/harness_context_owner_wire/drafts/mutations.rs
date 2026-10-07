use serde::{Deserialize, Serialize};

use super::super::boundary::{ContextBoundary, ContextOwnerBinding};
use super::super::digest::{HarnessDigestError, serialized_request_digest};
use super::super::validation::{
    MAX_HARNESS_DRAFT_OPERATIONS, MAX_HARNESS_NOTE_BYTES, MAX_HARNESS_OBJECTIVE_BYTES,
    OwnerWireError, validate_digest, validate_identifier, validate_schema,
};
use super::{
    CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1, CONTEXT_OWNER_DRAFT_REQUEST_SCHEMA_V1,
    CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1, CONTEXT_OWNER_MUTATION_RECEIPT_SCHEMA_V1,
    CONTEXT_OWNER_PREVIEW_REQUEST_SCHEMA_V1, ContextItemRef, HarnessContextOwnerDraftEnvelope,
    HarnessContextOwnerPreviewEnvelope,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftCreateRequest {
    pub schema_version: String,
    pub request_id: String,
    pub draft_id: String,
    pub base_revision_id: String,
    pub expected_boundary: ContextBoundary,
}

impl HarnessContextOwnerDraftCreateRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_DRAFT_REQUEST_SCHEMA_V1)?;
        validate_identifier("request_id", &self.request_id)?;
        validate_identifier("draft_id", &self.draft_id)?;
        validate_identifier("base_revision_id", &self.base_revision_id)?;
        self.expected_boundary.validate()
    }

    pub fn payload_digest(&self) -> Result<String, HarnessDigestError> {
        serialized_request_digest(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum HarnessContextOwnerDraftOperation {
    IncludeItem { reference: ContextItemRef },
    ExcludeItem { reference: ContextItemRef },
    PinItem { item_id: String },
    UnpinItem { item_id: String },
    PutNote { note_id: String, text: String },
    RemoveNote { note_id: String },
    SetObjective { text: String },
    RemoveObjective,
}

impl HarnessContextOwnerDraftOperation {
    fn validate(&self) -> Result<(), OwnerWireError> {
        match self {
            Self::IncludeItem { reference } | Self::ExcludeItem { reference } => {
                reference.validate()
            }
            Self::PinItem { item_id } | Self::UnpinItem { item_id } => {
                validate_identifier("operation_item_id", item_id)
            }
            Self::PutNote { note_id, text } => {
                validate_identifier("note_id", note_id)?;
                if text.is_empty() || text.len() > MAX_HARNESS_NOTE_BYTES || text.contains('\0') {
                    return Err(OwnerWireError::OutOfBounds("note_text"));
                }
                Ok(())
            }
            Self::RemoveNote { note_id } => validate_identifier("note_id", note_id),
            Self::SetObjective { text } => {
                if text.is_empty()
                    || text.len() > MAX_HARNESS_OBJECTIVE_BYTES
                    || text.contains('\0')
                {
                    return Err(OwnerWireError::OutOfBounds("objective_text"));
                }
                Ok(())
            }
            Self::RemoveObjective => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerDraftPatchRequest {
    pub schema_version: String,
    pub request_id: String,
    pub draft_id: String,
    pub expected_version: u64,
    pub expected_boundary: ContextBoundary,
    pub operations: Vec<HarnessContextOwnerDraftOperation>,
}

impl HarnessContextOwnerDraftPatchRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1)?;
        validate_identifier("request_id", &self.request_id)?;
        validate_identifier("draft_id", &self.draft_id)?;
        self.expected_boundary.validate()?;
        if self.expected_version == 0
            || self.operations.is_empty()
            || self.operations.len() > MAX_HARNESS_DRAFT_OPERATIONS
        {
            return Err(OwnerWireError::OutOfBounds("patch_request"));
        }
        for operation in &self.operations {
            operation.validate()?;
        }
        Ok(())
    }

    pub fn payload_digest(&self) -> Result<String, HarnessDigestError> {
        serialized_request_digest(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerPreviewRequest {
    pub schema_version: String,
    pub request_id: String,
    pub draft_id: String,
    pub expected_version: u64,
    pub expected_boundary: ContextBoundary,
}

impl HarnessContextOwnerPreviewRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_PREVIEW_REQUEST_SCHEMA_V1,
        )?;
        validate_identifier("request_id", &self.request_id)?;
        validate_identifier("draft_id", &self.draft_id)?;
        self.expected_boundary.validate()?;
        if self.expected_version == 0 {
            return Err(OwnerWireError::InvalidValue("preview_expected_version"));
        }
        Ok(())
    }

    pub fn payload_digest(&self) -> Result<String, HarnessDigestError> {
        serialized_request_digest(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "request",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HarnessContextOwnerMutationRequest {
    CreateDraft(HarnessContextOwnerDraftCreateRequest),
    PatchDraft(HarnessContextOwnerDraftPatchRequest),
    CreatePreview(HarnessContextOwnerPreviewRequest),
}

impl HarnessContextOwnerMutationRequest {
    #[must_use]
    pub fn request_id(&self) -> &str {
        match self {
            Self::CreateDraft(request) => &request.request_id,
            Self::PatchDraft(request) => &request.request_id,
            Self::CreatePreview(request) => &request.request_id,
        }
    }

    pub fn validate(&self) -> Result<(), OwnerWireError> {
        match self {
            Self::CreateDraft(request) => request.validate(),
            Self::PatchDraft(request) => request.validate(),
            Self::CreatePreview(request) => request.validate(),
        }
    }

    /// Harness hashes the concrete request struct, not this tagged recovery wrapper.
    pub fn payload_digest(&self) -> Result<String, HarnessDigestError> {
        match self {
            Self::CreateDraft(request) => request.payload_digest(),
            Self::PatchDraft(request) => request.payload_digest(),
            Self::CreatePreview(request) => request.payload_digest(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HarnessContextOwnerMutationResult {
    Draft(HarnessContextOwnerDraftEnvelope),
    Preview(HarnessContextOwnerPreviewEnvelope),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerMutationReceipt {
    pub schema_version: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub actor_subject: String,
    pub binding_id: String,
    pub binding_digest: String,
    pub invocation_id: String,
    pub boundary: ContextBoundary,
    pub operation: String,
    pub request_id: String,
    pub payload_digest: String,
    pub result: HarnessContextOwnerMutationResult,
    pub created_at: u64,
}

impl HarnessContextOwnerMutationReceipt {
    pub fn validate_for(
        &self,
        request: &HarnessContextOwnerMutationRequest,
        binding: &ContextOwnerBinding,
    ) -> Result<(), OwnerWireError> {
        self.schema_validate()?;
        binding.validate()?;
        request.validate()?;
        let (operation, expected_result) = match request {
            HarnessContextOwnerMutationRequest::CreateDraft(_) => ("create_draft", 0_u8),
            HarnessContextOwnerMutationRequest::PatchDraft(_) => ("patch_draft", 0_u8),
            HarnessContextOwnerMutationRequest::CreatePreview(_) => ("create_preview", 1_u8),
        };
        let expected_digest = request
            .payload_digest()
            .map_err(|_| OwnerWireError::JsonEncoding)?;
        if self.owner_id != binding.owner_id
            || self.workflow_run_id != binding.workflow_run_id
            || self.actor_subject.is_empty()
            || self.binding_id != binding.binding_id
            || self.binding_digest != binding.binding_digest
            || self.invocation_id != binding.invocation_id
            || self.boundary != binding.boundary
            || self.operation != operation
            || self.request_id != request.request_id()
            || self.payload_digest != expected_digest
            || self.created_at == 0
        {
            return Err(OwnerWireError::CorrelationMismatch("mutation_receipt"));
        }
        validate_digest("mutation_payload_digest", &self.payload_digest)?;
        match (&self.result, expected_result) {
            (HarnessContextOwnerMutationResult::Draft(draft), 0) => {
                draft.validate()?;
                if draft.actor_subject != self.actor_subject
                    || draft.binding != *binding
                    || draft.draft.draft_id != mutation_draft_id(request)
                {
                    return Err(OwnerWireError::CorrelationMismatch("mutation_draft_result"));
                }
            }
            (HarnessContextOwnerMutationResult::Preview(preview), 1) => {
                preview.validate()?;
                if preview.actor_subject != self.actor_subject
                    || preview.binding != *binding
                    || preview.draft_id != mutation_draft_id(request)
                {
                    return Err(OwnerWireError::CorrelationMismatch(
                        "mutation_preview_result",
                    ));
                }
            }
            _ => return Err(OwnerWireError::CorrelationMismatch("mutation_result_kind")),
        }
        Ok(())
    }

    fn schema_validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_MUTATION_RECEIPT_SCHEMA_V1,
        )?;
        for (field, value) in [
            ("mutation_owner_id", self.owner_id.as_str()),
            ("mutation_workflow_run_id", self.workflow_run_id.as_str()),
            ("mutation_actor_subject", self.actor_subject.as_str()),
            ("mutation_binding_id", self.binding_id.as_str()),
            ("mutation_invocation_id", self.invocation_id.as_str()),
            ("mutation_operation", self.operation.as_str()),
            ("mutation_request_id", self.request_id.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("mutation_binding_digest", &self.binding_digest)?;
        validate_digest("mutation_payload_digest", &self.payload_digest)?;
        self.boundary.validate()
    }
}

fn mutation_draft_id(request: &HarnessContextOwnerMutationRequest) -> &str {
    match request {
        HarnessContextOwnerMutationRequest::CreateDraft(request) => &request.draft_id,
        HarnessContextOwnerMutationRequest::PatchDraft(request) => &request.draft_id,
        HarnessContextOwnerMutationRequest::CreatePreview(request) => &request.draft_id,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessContextOwnerMutationLookupRequest {
    pub schema_version: String,
    pub request: HarnessContextOwnerMutationRequest,
}

impl HarnessContextOwnerMutationLookupRequest {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1,
        )?;
        self.request.validate()
    }
}
