use serde::{Deserialize, Serialize};

use super::super::boundary::{ContextBoundary, ContextOwnerBinding};
use super::super::control::ContextControlCommand;
use super::super::drafts::{
    HarnessContextOwnerDraftCreateRequest, HarnessContextOwnerDraftPatchRequest,
    HarnessContextOwnerMutationLookupRequest, HarnessContextOwnerMutationRequest,
    HarnessContextOwnerPreviewRequest,
};
use super::super::publication::{
    HarnessContextOwnerDraftPublicationLookupRequest, HarnessContextOwnerDraftPublicationRequest,
};
use super::super::source::{ContextSourceAdoptionRequest, ContextSourceUpload};
use super::super::validation::{OwnerWireError, validate_identifier};
use super::queries::{ContextOwnerItemsQueryV1, ContextOwnerRevisionQueryV1};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HarnessHttpMethod {
    Get,
    Post,
    Patch,
    Put,
}

impl HarnessHttpMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Put => "PUT",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessQuery {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerEndpointV1 {
    pub method: HarnessHttpMethod,
    pub path: String,
    pub query: Vec<HarnessQuery>,
}

/// A complete, typed operation set for the existing Harness owner routes. This enum is a local
/// Console journal/wire selector; its JSON representation is never sent as the Harness request
/// body. Each body-producing arm emits only its exact Harness v1 inner DTO.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextOwnerOperationV2 {
    CurrentAssociation {
        workflow_run_id: String,
    },
    CurrentSourceStatus {
        workflow_run_id: String,
    },
    CurrentEffectiveLimits {
        workflow_run_id: String,
    },
    EligibleItems {
        workflow_run_id: String,
        query: ContextOwnerItemsQueryV1,
    },
    DraftList {
        workflow_run_id: String,
    },
    GetDraft {
        workflow_run_id: String,
        draft_id: String,
    },
    CreateDraft {
        workflow_run_id: String,
        request: HarnessContextOwnerDraftCreateRequest,
    },
    PatchDraft {
        workflow_run_id: String,
        draft_id: String,
        request: HarnessContextOwnerDraftPatchRequest,
    },
    CreatePreview {
        workflow_run_id: String,
        draft_id: String,
        request: HarnessContextOwnerPreviewRequest,
    },
    RevisionPage {
        workflow_run_id: String,
        query: ContextOwnerRevisionQueryV1,
    },
    GetRevision {
        workflow_run_id: String,
        revision_id: String,
    },
    GetPreview {
        workflow_run_id: String,
        preview_id: String,
    },
    LookupMutation {
        workflow_run_id: String,
        request: HarnessContextOwnerMutationLookupRequest,
    },
    PublishedSources {
        workflow_run_id: String,
    },
    PublishDraft {
        workflow_run_id: String,
        draft_id: String,
        request: HarnessContextOwnerDraftPublicationRequest,
    },
    LookupPublication {
        workflow_run_id: String,
        request: HarnessContextOwnerDraftPublicationLookupRequest,
    },
    UploadSource {
        workflow_run_id: String,
        source_id: String,
        request: ContextSourceUpload,
    },
    AdoptSource {
        workflow_run_id: String,
        source_id: String,
        request: ContextSourceAdoptionRequest,
    },
    SubmitControl {
        workflow_run_id: String,
        command: ContextControlCommand,
    },
    LookupControl {
        workflow_run_id: String,
        command: ContextControlCommand,
    },
}

impl ContextOwnerOperationV2 {
    pub(super) fn workflow_run_id(&self) -> &str {
        match self {
            Self::CurrentAssociation { workflow_run_id }
            | Self::CurrentSourceStatus { workflow_run_id }
            | Self::CurrentEffectiveLimits { workflow_run_id }
            | Self::EligibleItems {
                workflow_run_id, ..
            }
            | Self::DraftList { workflow_run_id }
            | Self::GetDraft {
                workflow_run_id, ..
            }
            | Self::CreateDraft {
                workflow_run_id, ..
            }
            | Self::PatchDraft {
                workflow_run_id, ..
            }
            | Self::CreatePreview {
                workflow_run_id, ..
            }
            | Self::RevisionPage {
                workflow_run_id, ..
            }
            | Self::GetRevision {
                workflow_run_id, ..
            }
            | Self::GetPreview {
                workflow_run_id, ..
            }
            | Self::LookupMutation {
                workflow_run_id, ..
            }
            | Self::PublishedSources { workflow_run_id }
            | Self::PublishDraft {
                workflow_run_id, ..
            }
            | Self::LookupPublication {
                workflow_run_id, ..
            }
            | Self::UploadSource {
                workflow_run_id, ..
            }
            | Self::AdoptSource {
                workflow_run_id, ..
            }
            | Self::SubmitControl {
                workflow_run_id, ..
            }
            | Self::LookupControl {
                workflow_run_id, ..
            } => workflow_run_id,
        }
    }

    pub(super) fn validate(
        &self,
        binding: Option<&ContextOwnerBinding>,
    ) -> Result<(), OwnerWireError> {
        validate_identifier("operation_workflow_run_id", self.workflow_run_id())?;
        match self {
            Self::CurrentAssociation { .. }
            | Self::CurrentSourceStatus { .. }
            | Self::DraftList { .. }
            | Self::PublishedSources { .. } => {}
            Self::CurrentEffectiveLimits { .. } => {
                binding.ok_or(OwnerWireError::InvalidValue("expected_binding"))?;
            }
            Self::EligibleItems { query, .. } => query.validate()?,
            Self::GetDraft { draft_id, .. }
            | Self::GetPreview {
                preview_id: draft_id,
                ..
            }
            | Self::GetRevision {
                revision_id: draft_id,
                ..
            } => validate_identifier("operation_object_id", draft_id)?,
            Self::CreateDraft {
                workflow_run_id,
                request,
            } => {
                request.validate()?;
                validate_request_binding(workflow_run_id, &request.expected_boundary, binding)?;
            }
            Self::PatchDraft {
                workflow_run_id,
                draft_id,
                request,
            } => {
                validate_identifier("patch_path_draft_id", draft_id)?;
                request.validate()?;
                if request.draft_id != *draft_id {
                    return Err(OwnerWireError::CorrelationMismatch("patch_path_draft_id"));
                }
                validate_request_binding(workflow_run_id, &request.expected_boundary, binding)?;
            }
            Self::CreatePreview {
                workflow_run_id,
                draft_id,
                request,
            } => {
                validate_identifier("preview_path_draft_id", draft_id)?;
                request.validate()?;
                if request.draft_id != *draft_id {
                    return Err(OwnerWireError::CorrelationMismatch("preview_path_draft_id"));
                }
                validate_request_binding(workflow_run_id, &request.expected_boundary, binding)?;
            }
            Self::RevisionPage { query, .. } => query.validate()?,
            Self::LookupMutation {
                workflow_run_id,
                request,
            } => {
                request.validate()?;
                let boundary = mutation_boundary(&request.request);
                validate_request_binding(workflow_run_id, boundary, binding)?;
            }
            Self::PublishDraft {
                workflow_run_id,
                draft_id,
                request,
            } => {
                validate_identifier("publish_path_draft_id", draft_id)?;
                request.validate()?;
                if request.draft_id != *draft_id {
                    return Err(OwnerWireError::CorrelationMismatch("publish_path_draft_id"));
                }
                validate_request_binding(workflow_run_id, &request.expected_boundary, binding)?;
                let binding = binding.ok_or(OwnerWireError::InvalidValue("expected_binding"))?;
                if request.expected_binding_id != binding.binding_id
                    || request.expected_binding_digest != binding.binding_digest
                {
                    return Err(OwnerWireError::CorrelationMismatch("publication_binding"));
                }
            }
            Self::LookupPublication {
                workflow_run_id,
                request,
            } => {
                request.validate()?;
                validate_request_binding(
                    workflow_run_id,
                    &request.request.expected_boundary,
                    binding,
                )?;
            }
            Self::UploadSource {
                workflow_run_id,
                source_id,
                request,
            } => {
                validate_identifier("upload_path_source_id", source_id)?;
                request.validate()?;
                let binding = binding.ok_or(OwnerWireError::InvalidValue("expected_binding"))?;
                if binding.workflow_run_id != *workflow_run_id {
                    return Err(OwnerWireError::CorrelationMismatch("upload_run_id"));
                }
            }
            Self::AdoptSource {
                workflow_run_id,
                source_id,
                request,
            } => {
                validate_identifier("adopt_path_source_id", source_id)?;
                request.validate()?;
                validate_request_binding(workflow_run_id, &request.expected_boundary, binding)?;
            }
            Self::SubmitControl {
                workflow_run_id,
                command,
            }
            | Self::LookupControl {
                workflow_run_id,
                command,
            } => {
                let binding = binding.ok_or(OwnerWireError::InvalidValue("expected_binding"))?;
                if binding.workflow_run_id != *workflow_run_id {
                    return Err(OwnerWireError::CorrelationMismatch("control_run_id"));
                }
                command.validate(binding)?;
            }
        }
        if binding.is_some_and(|binding| binding.workflow_run_id != self.workflow_run_id()) {
            return Err(OwnerWireError::CorrelationMismatch("binding_run_id"));
        }
        Ok(())
    }
}
fn validate_request_binding(
    workflow_run_id: &str,
    boundary: &ContextBoundary,
    binding: Option<&ContextOwnerBinding>,
) -> Result<(), OwnerWireError> {
    boundary.validate()?;
    if boundary.run_id != workflow_run_id {
        return Err(OwnerWireError::CorrelationMismatch("request_boundary_run"));
    }
    let binding = binding.ok_or(OwnerWireError::InvalidValue("expected_binding"))?;
    if boundary != &binding.boundary {
        return Err(OwnerWireError::CorrelationMismatch("request_boundary"));
    }
    Ok(())
}

fn mutation_boundary(request: &HarnessContextOwnerMutationRequest) -> &ContextBoundary {
    match request {
        HarnessContextOwnerMutationRequest::CreateDraft(request) => &request.expected_boundary,
        HarnessContextOwnerMutationRequest::PatchDraft(request) => &request.expected_boundary,
        HarnessContextOwnerMutationRequest::CreatePreview(request) => &request.expected_boundary,
    }
}
