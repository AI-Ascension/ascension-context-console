use super::super::validation::{OwnerWireError, bounded_json_bytes};
use super::types::{
    ContextOwnerEndpointV1, ContextOwnerOperationV2, HarnessHttpMethod, HarnessQuery,
};
impl ContextOwnerOperationV2 {
    pub fn endpoint(&self) -> Result<ContextOwnerEndpointV1, OwnerWireError> {
        self.validate(None).or_else(|error| {
            // Mutations require a binding at the call envelope, but their path is still safe to
            // derive after validating every path/query component here.
            if matches!(error, OwnerWireError::InvalidValue("expected_binding")) {
                Ok(())
            } else {
                Err(error)
            }
        })?;
        let run = self.workflow_run_id();
        let prefix = format!("/v1/workflow-runs/{run}");
        let endpoint = match self {
            Self::CurrentAssociation { .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-association"),
                Vec::new(),
            ),
            Self::CurrentSourceStatus { .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-source-status"),
                Vec::new(),
            ),
            Self::CurrentEffectiveLimits { .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-effective-limits"),
                Vec::new(),
            ),
            Self::EligibleItems { query, .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-items"),
                query.query(),
            ),
            Self::DraftList { .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-drafts"),
                Vec::new(),
            ),
            Self::GetDraft { draft_id, .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-drafts/{draft_id}"),
                Vec::new(),
            ),
            Self::CreateDraft { .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-owner-drafts"),
                Vec::new(),
            ),
            Self::PatchDraft { draft_id, .. } => endpoint(
                HarnessHttpMethod::Patch,
                format!("{prefix}/context-owner-drafts/{draft_id}"),
                Vec::new(),
            ),
            Self::CreatePreview { draft_id, .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-owner-drafts/{draft_id}/previews"),
                Vec::new(),
            ),
            Self::RevisionPage { query, .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-revisions"),
                query.query(),
            ),
            Self::GetRevision { revision_id, .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-revisions/{revision_id}"),
                Vec::new(),
            ),
            Self::GetPreview { preview_id, .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-previews/{preview_id}"),
                Vec::new(),
            ),
            Self::LookupMutation { .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-owner-mutation-receipts/lookup"),
                Vec::new(),
            ),
            Self::PublishedSources { .. } => endpoint(
                HarnessHttpMethod::Get,
                format!("{prefix}/context-owner-published-sources"),
                Vec::new(),
            ),
            Self::PublishDraft { draft_id, .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-owner-drafts/{draft_id}/publications"),
                Vec::new(),
            ),
            Self::LookupPublication { .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-owner-draft-publication-receipts/lookup"),
                Vec::new(),
            ),
            Self::UploadSource { source_id, .. } => endpoint(
                HarnessHttpMethod::Put,
                format!("{prefix}/context-sources/{source_id}"),
                Vec::new(),
            ),
            Self::AdoptSource { source_id, .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-sources/{source_id}/adopt"),
                Vec::new(),
            ),
            Self::SubmitControl { .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-control-commands"),
                Vec::new(),
            ),
            Self::LookupControl { .. } => endpoint(
                HarnessHttpMethod::Post,
                format!("{prefix}/context-control-receipts/lookup"),
                Vec::new(),
            ),
        };
        Ok(endpoint)
    }

    /// Returns only the exact inner Harness body. Local Console principal, scope, and grant
    /// correlation fields never enter the owner JSON.
    pub fn harness_body(&self) -> Result<Option<Vec<u8>>, OwnerWireError> {
        let body = match self {
            Self::CreateDraft { request, .. } => Some(bounded_json_bytes(request)?),
            Self::PatchDraft { request, .. } => Some(bounded_json_bytes(request)?),
            Self::CreatePreview { request, .. } => Some(bounded_json_bytes(request)?),
            Self::LookupMutation { request, .. } => Some(bounded_json_bytes(request)?),
            Self::PublishDraft { request, .. } => Some(bounded_json_bytes(request)?),
            Self::LookupPublication { request, .. } => Some(bounded_json_bytes(request)?),
            Self::UploadSource { request, .. } => Some(bounded_json_bytes(request)?),
            Self::AdoptSource { request, .. } => Some(bounded_json_bytes(request)?),
            Self::SubmitControl { command, .. } | Self::LookupControl { command, .. } => {
                Some(bounded_json_bytes(command)?)
            }
            _ => None,
        };
        Ok(body)
    }

    /// Harness mutation and publication receipts use these exact digest contracts. Other owner
    /// operations are correlated by their complete typed request, not an invented digest field.
    pub fn harness_request_digest(&self) -> Result<Option<String>, OwnerWireError> {
        match self {
            Self::CreateDraft { request, .. } => request
                .payload_digest()
                .map(Some)
                .map_err(|_| OwnerWireError::JsonEncoding),
            Self::PatchDraft { request, .. } => request
                .payload_digest()
                .map(Some)
                .map_err(|_| OwnerWireError::JsonEncoding),
            Self::CreatePreview { request, .. } => request
                .payload_digest()
                .map(Some)
                .map_err(|_| OwnerWireError::JsonEncoding),
            Self::PublishDraft { request, .. } => request.request_digest().map(Some),
            _ => Ok(None),
        }
    }
}
fn endpoint(
    method: HarnessHttpMethod,
    path: String,
    query: Vec<HarnessQuery>,
) -> ContextOwnerEndpointV1 {
    ContextOwnerEndpointV1 {
        method,
        path,
        query,
    }
}
