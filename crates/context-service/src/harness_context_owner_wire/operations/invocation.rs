use serde::{Deserialize, Serialize};

use super::super::boundary::{
    ContextBoundary, ContextOwnerAssociationView, ContextOwnerBinding,
    ContextOwnerEffectiveLimitsView,
};
use super::super::control::ContextControlReceipt;
use super::super::drafts::{
    HarnessContextOwnerDraftEnvelope, HarnessContextOwnerDraftListView,
    HarnessContextOwnerItemsView, HarnessContextOwnerMutationReceipt,
    HarnessContextOwnerMutationRequest, HarnessContextOwnerPreviewEnvelope,
    HarnessContextOwnerRevisionEnvelope, HarnessContextOwnerRevisionPage,
};
use super::super::identity::{
    CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2, OwnerIdentityCorrelationV2,
};
use super::super::publication::{
    HarnessContextOwnerDraftPublicationReceipt, HarnessContextOwnerPublishedSourcesView,
};
use super::super::source::{ContextOwnerSourceStatus, ContextSourcePublication};
use super::super::validation::{OwnerWireError, decode_bounded_json, validate_schema};
use super::types::{ContextOwnerEndpointV1, ContextOwnerOperationV2};
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerInvocationV2 {
    pub schema_version: String,
    pub identity: OwnerIdentityCorrelationV2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_binding: Option<ContextOwnerBinding>,
    pub operation: ContextOwnerOperationV2,
}

impl ContextOwnerInvocationV2 {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2,
        )?;
        self.identity.validate()?;
        if let Some(binding) = &self.expected_binding {
            binding.validate()?;
            if binding.owner_id != self.identity.harness.owner_id
                || binding.workflow_run_id != self.identity.harness.workflow_run_id
                || binding.workflow_run_id != self.identity.console_scope.run_id
            {
                return Err(OwnerWireError::CorrelationMismatch("expected_binding"));
            }
            check_boundary_scope(&binding.boundary, &self.identity)?;
        }
        self.operation.validate(self.expected_binding.as_ref())?;
        if self.operation.workflow_run_id() != self.identity.console_scope.run_id {
            return Err(OwnerWireError::CorrelationMismatch("operation_scope"));
        }
        let _ = self.operation.harness_body()?;
        Ok(())
    }

    pub fn endpoint(&self) -> Result<ContextOwnerEndpointV1, OwnerWireError> {
        self.validate()?;
        self.operation.endpoint()
    }

    pub fn harness_body(&self) -> Result<Option<Vec<u8>>, OwnerWireError> {
        self.validate()?;
        self.operation.harness_body()
    }

    pub fn decode_response(&self, bytes: &[u8]) -> Result<HarnessResponseV1, OwnerWireError> {
        self.validate()?;
        let expected_binding = self.expected_binding.as_ref();
        let run_id = self.identity.console_scope.run_id.as_str();
        let response = match &self.operation {
            ContextOwnerOperationV2::CurrentAssociation { .. } => {
                let value: ContextOwnerAssociationView = decode_bounded_json(bytes)?;
                value.validate()?;
                check_binding_scope(&value.binding, run_id, &self.identity)?;
                HarnessResponseV1::Association(value)
            }
            ContextOwnerOperationV2::CurrentSourceStatus { .. } => {
                let value: ContextOwnerSourceStatus = decode_bounded_json(bytes)?;
                value.validate(run_id)?;
                check_owner_scope(&value.owner_id, run_id, &self.identity)?;
                check_boundary_scope(&value.boundary, &self.identity)?;
                HarnessResponseV1::SourceStatus(value)
            }
            ContextOwnerOperationV2::CurrentEffectiveLimits { .. } => {
                let value: ContextOwnerEffectiveLimitsView = decode_bounded_json(bytes)?;
                value.validate()?;
                let binding = required_binding(expected_binding)?;
                check_owner_scope(&value.owner_id, run_id, &self.identity)?;
                check_effective_limits_binding(&value, binding)?;
                HarnessResponseV1::EffectiveLimits(value)
            }
            ContextOwnerOperationV2::EligibleItems { .. } => {
                let value: HarnessContextOwnerItemsView = decode_bounded_json(bytes)?;
                value.validate()?;
                check_owner_scope(&value.owner_id, run_id, &self.identity)?;
                check_boundary_scope(&value.boundary, &self.identity)?;
                if let Some(binding) = expected_binding
                    && (value.binding_id != binding.binding_id
                        || value.binding_digest != binding.binding_digest
                        || value.boundary != binding.boundary)
                {
                    return Err(OwnerWireError::CorrelationMismatch("items_binding"));
                }
                HarnessResponseV1::Items(value)
            }
            ContextOwnerOperationV2::DraftList { .. } => {
                let value: HarnessContextOwnerDraftListView = decode_bounded_json(bytes)?;
                value.validate()?;
                check_owner_scope(&value.owner_id, &value.workflow_run_id, &self.identity)?;
                if value.workflow_run_id != run_id {
                    return Err(OwnerWireError::CorrelationMismatch("draft_list_run"));
                }
                for draft in &value.drafts {
                    check_binding_scope(&draft.binding, run_id, &self.identity)?;
                    check_actor(&draft.actor_subject, &self.identity)?;
                }
                HarnessResponseV1::DraftList(value)
            }
            ContextOwnerOperationV2::GetDraft { draft_id, .. } => {
                let value: HarnessContextOwnerDraftEnvelope = decode_bounded_json(bytes)?;
                value.validate()?;
                check_binding_scope(&value.binding, run_id, &self.identity)?;
                check_actor(&value.actor_subject, &self.identity)?;
                if value.draft.draft_id != *draft_id {
                    return Err(OwnerWireError::CorrelationMismatch("draft_id"));
                }
                HarnessResponseV1::Draft(value)
            }
            ContextOwnerOperationV2::CreateDraft { request, .. } => {
                let value: HarnessContextOwnerMutationReceipt = decode_bounded_json(bytes)?;
                value.validate_for(
                    &HarnessContextOwnerMutationRequest::CreateDraft(request.clone()),
                    required_binding(expected_binding)?,
                )?;
                check_actor(&value.actor_subject, &self.identity)?;
                HarnessResponseV1::MutationReceipt(value)
            }
            ContextOwnerOperationV2::PatchDraft { request, .. } => {
                let value: HarnessContextOwnerMutationReceipt = decode_bounded_json(bytes)?;
                value.validate_for(
                    &HarnessContextOwnerMutationRequest::PatchDraft(request.clone()),
                    required_binding(expected_binding)?,
                )?;
                check_actor(&value.actor_subject, &self.identity)?;
                HarnessResponseV1::MutationReceipt(value)
            }
            ContextOwnerOperationV2::CreatePreview { request, .. } => {
                let value: HarnessContextOwnerMutationReceipt = decode_bounded_json(bytes)?;
                value.validate_for(
                    &HarnessContextOwnerMutationRequest::CreatePreview(request.clone()),
                    required_binding(expected_binding)?,
                )?;
                check_actor(&value.actor_subject, &self.identity)?;
                HarnessResponseV1::MutationReceipt(value)
            }
            ContextOwnerOperationV2::RevisionPage { .. } => {
                let value: HarnessContextOwnerRevisionPage = decode_bounded_json(bytes)?;
                value.validate()?;
                check_owner_scope(&value.owner_id, &value.workflow_run_id, &self.identity)?;
                if value.workflow_run_id != run_id {
                    return Err(OwnerWireError::CorrelationMismatch("revision_page_run"));
                }
                for revision in &value.revisions {
                    check_binding_scope(&revision.binding, run_id, &self.identity)?;
                    check_actor(&revision.actor_subject, &self.identity)?;
                }
                HarnessResponseV1::RevisionPage(value)
            }
            ContextOwnerOperationV2::GetRevision { revision_id, .. } => {
                let value: HarnessContextOwnerRevisionEnvelope = decode_bounded_json(bytes)?;
                value.validate()?;
                check_binding_scope(&value.binding, run_id, &self.identity)?;
                check_actor(&value.actor_subject, &self.identity)?;
                if value.revision_id != *revision_id {
                    return Err(OwnerWireError::CorrelationMismatch("revision_id"));
                }
                HarnessResponseV1::Revision(value)
            }
            ContextOwnerOperationV2::GetPreview { preview_id, .. } => {
                let value: HarnessContextOwnerPreviewEnvelope = decode_bounded_json(bytes)?;
                value.validate()?;
                check_binding_scope(&value.binding, run_id, &self.identity)?;
                check_actor(&value.actor_subject, &self.identity)?;
                if value.preview_id != *preview_id {
                    return Err(OwnerWireError::CorrelationMismatch("preview_id"));
                }
                HarnessResponseV1::Preview(value)
            }
            ContextOwnerOperationV2::LookupMutation { request, .. } => {
                let value: Option<HarnessContextOwnerMutationReceipt> = decode_bounded_json(bytes)?;
                if let Some(receipt) = &value {
                    receipt.validate_for(&request.request, required_binding(expected_binding)?)?;
                    check_actor(&receipt.actor_subject, &self.identity)?;
                }
                HarnessResponseV1::MutationLookup(value)
            }
            ContextOwnerOperationV2::PublishedSources { .. } => {
                let value: HarnessContextOwnerPublishedSourcesView = decode_bounded_json(bytes)?;
                value.validate()?;
                check_binding_scope(&value.binding, run_id, &self.identity)?;
                HarnessResponseV1::PublishedSources(value)
            }
            ContextOwnerOperationV2::PublishDraft { request, .. } => {
                let value: HarnessContextOwnerDraftPublicationReceipt = decode_bounded_json(bytes)?;
                value.validate_for(request, &self.identity.console.subject, run_id)?;
                HarnessResponseV1::PublicationReceipt(value)
            }
            ContextOwnerOperationV2::LookupPublication { request, .. } => {
                let value: Option<HarnessContextOwnerDraftPublicationReceipt> =
                    decode_bounded_json(bytes)?;
                if let Some(receipt) = &value {
                    receipt.validate_for(
                        &request.request,
                        &self.identity.console.subject,
                        run_id,
                    )?;
                }
                HarnessResponseV1::PublicationLookup(value)
            }
            ContextOwnerOperationV2::UploadSource {
                source_id, request, ..
            } => {
                let value: ContextSourcePublication = decode_bounded_json(bytes)?;
                value.validate()?;
                if value.source.source_id != *source_id
                    || value.source.digest != request.document_digest()?
                {
                    return Err(OwnerWireError::CorrelationMismatch("uploaded_source_id"));
                }
                HarnessResponseV1::SourcePublication(value)
            }
            ContextOwnerOperationV2::AdoptSource { request, .. } => {
                let value: ContextControlReceipt = decode_bounded_json(bytes)?;
                value.validate_adoption_for(required_binding(expected_binding)?, request)?;
                HarnessResponseV1::ControlReceipt(value)
            }
            ContextOwnerOperationV2::SubmitControl { command, .. }
            | ContextOwnerOperationV2::LookupControl { command, .. } => {
                let value: ContextControlReceipt = decode_bounded_json(bytes)?;
                value.validate_for(required_binding(expected_binding)?, command)?;
                HarnessResponseV1::ControlReceipt(value)
            }
        };
        Ok(response)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HarnessResponseV1 {
    Association(ContextOwnerAssociationView),
    SourceStatus(ContextOwnerSourceStatus),
    EffectiveLimits(ContextOwnerEffectiveLimitsView),
    Items(HarnessContextOwnerItemsView),
    DraftList(HarnessContextOwnerDraftListView),
    Draft(HarnessContextOwnerDraftEnvelope),
    MutationReceipt(HarnessContextOwnerMutationReceipt),
    MutationLookup(Option<HarnessContextOwnerMutationReceipt>),
    Preview(HarnessContextOwnerPreviewEnvelope),
    RevisionPage(HarnessContextOwnerRevisionPage),
    Revision(HarnessContextOwnerRevisionEnvelope),
    PublishedSources(HarnessContextOwnerPublishedSourcesView),
    PublicationReceipt(HarnessContextOwnerDraftPublicationReceipt),
    PublicationLookup(Option<HarnessContextOwnerDraftPublicationReceipt>),
    SourcePublication(ContextSourcePublication),
    ControlReceipt(ContextControlReceipt),
}

fn required_binding(
    binding: Option<&ContextOwnerBinding>,
) -> Result<&ContextOwnerBinding, OwnerWireError> {
    binding.ok_or(OwnerWireError::InvalidValue("expected_binding"))
}

fn check_owner_scope(
    owner_id: &str,
    run_id: &str,
    identity: &OwnerIdentityCorrelationV2,
) -> Result<(), OwnerWireError> {
    if owner_id != identity.harness.owner_id || run_id != identity.harness.workflow_run_id {
        return Err(OwnerWireError::CorrelationMismatch("owner_scope"));
    }
    Ok(())
}

fn check_binding_scope(
    binding: &ContextOwnerBinding,
    run_id: &str,
    identity: &OwnerIdentityCorrelationV2,
) -> Result<(), OwnerWireError> {
    binding.validate()?;
    check_owner_scope(&binding.owner_id, run_id, identity)?;
    check_boundary_scope(&binding.boundary, identity)
}

fn check_effective_limits_binding(
    view: &ContextOwnerEffectiveLimitsView,
    binding: &ContextOwnerBinding,
) -> Result<(), OwnerWireError> {
    if view.owner_id != binding.owner_id
        || view.owner_version != binding.owner_version
        || view.binding_id != binding.binding_id
        || view.binding_version != binding.binding_version
        || view.binding_digest != binding.binding_digest
        || view.context_ref != binding.context_ref
        || view.node_kind != binding.node_kind
        || view.adapter_revision != binding.boundary.adapter_revision
        || view.model_revision != binding.boundary.model_revision
    {
        return Err(OwnerWireError::CorrelationMismatch(
            "effective_limits_binding",
        ));
    }
    Ok(())
}

fn check_boundary_scope(
    boundary: &ContextBoundary,
    identity: &OwnerIdentityCorrelationV2,
) -> Result<(), OwnerWireError> {
    if boundary.run_id != identity.console_scope.run_id
        || boundary.episode_id != identity.console_scope.episode_id
        || boundary.agent_id != identity.console_scope.agent_id
    {
        return Err(OwnerWireError::CorrelationMismatch(
            "console_scope_boundary",
        ));
    }
    Ok(())
}

fn check_actor(
    actor_subject: &str,
    identity: &OwnerIdentityCorrelationV2,
) -> Result<(), OwnerWireError> {
    if actor_subject != identity.harness.actor_subject || actor_subject != identity.console.subject
    {
        return Err(OwnerWireError::CorrelationMismatch("receipt_actor"));
    }
    Ok(())
}
