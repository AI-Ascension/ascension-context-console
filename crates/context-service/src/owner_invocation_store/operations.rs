use crate::control::Scope;
use crate::harness_context_owner_wire::{
    CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1, CONTEXT_OWNER_PUBLICATION_LOOKUP_SCHEMA_V1,
    ContextControlCommand, ContextOwnerInvocationV2, ContextOwnerOperationV2,
    HarnessContextOwnerDraftPublicationLookupRequest, HarnessContextOwnerMutationLookupRequest,
    HarnessContextOwnerMutationRequest, HarnessResponseV1,
};
use crate::harness_facade::FacadePermission;
use crate::subject_grants::AdmittedSubjectGrant;

use super::record::{
    AdmissionError, AdmissionSnapshot, AdmissionUse, GrantSnapshot, LookupFamily, StableLocator,
};

fn operation_supports_use(operation: &ContextOwnerOperationV2, use_kind: AdmissionUse) -> bool {
    use ContextOwnerOperationV2 as Operation;

    match use_kind {
        AdmissionUse::Write | AdmissionUse::CachedRead => matches!(
            operation,
            Operation::CreateDraft { .. }
                | Operation::PatchDraft { .. }
                | Operation::CreatePreview { .. }
                | Operation::PublishDraft { .. }
                | Operation::UploadSource { .. }
                | Operation::SubmitControl { .. }
        ),
        AdmissionUse::ExactLookup => matches!(
            operation,
            Operation::CreateDraft { .. }
                | Operation::PatchDraft { .. }
                | Operation::CreatePreview { .. }
                | Operation::PublishDraft { .. }
                | Operation::SubmitControl { .. }
        ),
        AdmissionUse::ReadOnly => matches!(
            operation,
            Operation::CurrentAssociation { .. }
                | Operation::CurrentSourceStatus { .. }
                | Operation::CurrentEffectiveLimits { .. }
                | Operation::EligibleItems { .. }
                | Operation::DraftList { .. }
                | Operation::GetDraft { .. }
                | Operation::RevisionPage { .. }
                | Operation::GetRevision { .. }
                | Operation::GetPreview { .. }
                | Operation::PublishedSources { .. }
        ),
    }
}

pub(super) fn stable_locator(
    invocation: &ContextOwnerInvocationV2,
) -> Result<StableLocator, AdmissionError> {
    let identity = &invocation.identity;
    let (family, stable_key) = match &invocation.operation {
        ContextOwnerOperationV2::CreateDraft { request, .. } => {
            (LookupFamily::Mutation, request.request_id.clone())
        }
        ContextOwnerOperationV2::PatchDraft { request, .. } => {
            (LookupFamily::Mutation, request.request_id.clone())
        }
        ContextOwnerOperationV2::CreatePreview { request, .. } => {
            (LookupFamily::Mutation, request.request_id.clone())
        }
        ContextOwnerOperationV2::SubmitControl { command, .. } => {
            (LookupFamily::Control, command.idempotency_key().to_owned())
        }
        ContextOwnerOperationV2::PublishDraft { request, .. } => {
            (LookupFamily::Publication, request.request_id.clone())
        }
        ContextOwnerOperationV2::UploadSource { source_id, .. } => {
            (LookupFamily::SourceUpload, source_id.clone())
        }
        ContextOwnerOperationV2::AdoptSource { .. } => {
            return Err(AdmissionError::UnsupportedOperation);
        }
        _ => return Err(AdmissionError::UnsupportedOperation),
    };
    Ok(StableLocator {
        family,
        stable_key,
        issuer: identity.console.issuer.clone(),
        subject: identity.console.subject.clone(),
        audience: identity.console.audience.clone(),
        scope: Scope {
            project_id: identity.console_scope.project_id.clone(),
            run_id: identity.console_scope.run_id.clone(),
            episode_id: identity.console_scope.episode_id.clone(),
            agent_id: identity.console_scope.agent_id.clone(),
        },
    })
}

pub(super) fn same_call(
    stored: &ContextOwnerInvocationV2,
    current: &ContextOwnerInvocationV2,
) -> bool {
    let mut normalized = current.clone();
    normalized.identity.console.credential_id = stored.identity.console.credential_id.clone();
    normalized.identity.console.grant_id = stored.identity.console.grant_id.clone();
    normalized.identity.console.grant_generation = stored.identity.console.grant_generation;
    normalized.identity.console.grant_expires_at = stored.identity.console.grant_expires_at;
    normalized.identity.harness.credential_reference_id =
        stored.identity.harness.credential_reference_id.clone();
    normalized.identity.harness.credential_expires_at =
        stored.identity.harness.credential_expires_at;
    &normalized == stored
}

pub(super) fn admission_matches(original: &AdmissionSnapshot, current: &AdmissionSnapshot) -> bool {
    original.issuer == current.issuer
        && original.subject == current.subject
        && original.audience == current.audience
        && original.scope == current.scope
        && original.harness_actor == current.harness_actor
        && original.harness_owner == current.harness_owner
        && original.harness_workflow_run == current.harness_workflow_run
        && original.binding == current.binding
}

pub(crate) fn lookup_invocation(
    original: &ContextOwnerInvocationV2,
    current: &ContextOwnerInvocationV2,
) -> Result<ContextOwnerInvocationV2, AdmissionError> {
    if !same_call(original, current) {
        return Err(AdmissionError::Denied);
    }
    let mut lookup = current.clone();
    lookup.operation = match &original.operation {
        ContextOwnerOperationV2::CreateDraft {
            workflow_run_id,
            request,
        } => ContextOwnerOperationV2::LookupMutation {
            workflow_run_id: workflow_run_id.clone(),
            request: HarnessContextOwnerMutationLookupRequest {
                schema_version: CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1.to_owned(),
                request: HarnessContextOwnerMutationRequest::CreateDraft(request.clone()),
            },
        },
        ContextOwnerOperationV2::PatchDraft {
            workflow_run_id,
            request,
            ..
        } => ContextOwnerOperationV2::LookupMutation {
            workflow_run_id: workflow_run_id.clone(),
            request: HarnessContextOwnerMutationLookupRequest {
                schema_version: CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1.to_owned(),
                request: HarnessContextOwnerMutationRequest::PatchDraft(request.clone()),
            },
        },
        ContextOwnerOperationV2::CreatePreview {
            workflow_run_id,
            request,
            ..
        } => ContextOwnerOperationV2::LookupMutation {
            workflow_run_id: workflow_run_id.clone(),
            request: HarnessContextOwnerMutationLookupRequest {
                schema_version: CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1.to_owned(),
                request: HarnessContextOwnerMutationRequest::CreatePreview(request.clone()),
            },
        },
        ContextOwnerOperationV2::SubmitControl {
            workflow_run_id,
            command,
        } => ContextOwnerOperationV2::LookupControl {
            workflow_run_id: workflow_run_id.clone(),
            command: command.clone(),
        },
        ContextOwnerOperationV2::PublishDraft {
            workflow_run_id,
            request,
            ..
        } => ContextOwnerOperationV2::LookupPublication {
            workflow_run_id: workflow_run_id.clone(),
            request: HarnessContextOwnerDraftPublicationLookupRequest {
                schema_version: CONTEXT_OWNER_PUBLICATION_LOOKUP_SCHEMA_V1.to_owned(),
                request: request.clone(),
            },
        },
        _ => return Err(AdmissionError::UnsupportedOperation),
    };
    lookup.validate().map_err(|_| AdmissionError::Denied)?;
    Ok(lookup)
}

pub(crate) fn required_console_permissions(
    operation: &ContextOwnerOperationV2,
    use_kind: AdmissionUse,
) -> Result<Vec<FacadePermission>, AdmissionError> {
    if !operation_supports_use(operation, use_kind) {
        return Err(AdmissionError::UnsupportedOperation);
    }
    if matches!(
        use_kind,
        AdmissionUse::ExactLookup | AdmissionUse::CachedRead
    ) {
        return Ok(vec![FacadePermission::MetadataRead]);
    }
    if use_kind == AdmissionUse::ReadOnly {
        return Ok(match operation {
            ContextOwnerOperationV2::EligibleItems {
                query:
                    crate::harness_context_owner_wire::ContextOwnerItemsQueryV1 {
                        include_content: Some(true),
                        ..
                    },
                ..
            } => vec![
                FacadePermission::MetadataRead,
                FacadePermission::ContentRead,
            ],
            _ => vec![FacadePermission::MetadataRead],
        });
    }
    let mut values = match operation {
        ContextOwnerOperationV2::CreateDraft { .. }
        | ContextOwnerOperationV2::PatchDraft { .. }
        | ContextOwnerOperationV2::CreatePreview { .. } => vec![FacadePermission::Edit],
        ContextOwnerOperationV2::PublishDraft { .. }
        | ContextOwnerOperationV2::UploadSource { .. } => vec![FacadePermission::ContentWrite],
        ContextOwnerOperationV2::SubmitControl { command, .. } => vec![match command {
            ContextControlCommand::Pause { .. } => FacadePermission::Pause,
            ContextControlCommand::Commit { .. } => FacadePermission::Commit,
            ContextControlCommand::Resume { .. } => FacadePermission::Resume,
        }],
        _ => return Err(AdmissionError::UnsupportedOperation),
    };
    if let ContextOwnerOperationV2::PatchDraft { request, .. } = operation
        && request.operations.iter().any(|operation| {
            matches!(
                operation,
                crate::harness_context_owner_wire::HarnessContextOwnerDraftOperation::SetObjective { .. }
                    | crate::harness_context_owner_wire::HarnessContextOwnerDraftOperation::RemoveObjective
            )
        })
    {
        values.push(FacadePermission::Objective);
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

pub(crate) fn required_harness_scopes(
    operation: &ContextOwnerOperationV2,
    use_kind: AdmissionUse,
) -> Result<Vec<&'static str>, AdmissionError> {
    if !operation_supports_use(operation, use_kind) {
        return Err(AdmissionError::UnsupportedOperation);
    }
    if matches!(
        use_kind,
        AdmissionUse::ExactLookup | AdmissionUse::CachedRead
    ) {
        return Ok(vec!["workflow:read"]);
    }
    if use_kind == AdmissionUse::ReadOnly {
        return Ok(match operation {
            ContextOwnerOperationV2::EligibleItems {
                query:
                    crate::harness_context_owner_wire::ContextOwnerItemsQueryV1 {
                        include_content: Some(true),
                        ..
                    },
                ..
            } => vec!["workflow:read", "workflow:context:content:read"],
            _ => vec!["workflow:read"],
        });
    }
    let mut values = match operation {
        ContextOwnerOperationV2::CreateDraft { .. }
        | ContextOwnerOperationV2::PatchDraft { .. }
        | ContextOwnerOperationV2::CreatePreview { .. } => vec!["workflow:context:edit"],
        ContextOwnerOperationV2::PublishDraft { .. }
        | ContextOwnerOperationV2::UploadSource { .. } => vec!["workflow:content:write"],
        ContextOwnerOperationV2::SubmitControl { .. } => vec!["workflow:control"],
        _ => return Err(AdmissionError::UnsupportedOperation),
    };
    if let ContextOwnerOperationV2::PatchDraft { request, .. } = operation
        && request.operations.iter().any(|operation| {
            matches!(
                operation,
                crate::harness_context_owner_wire::HarnessContextOwnerDraftOperation::SetObjective { .. }
                    | crate::harness_context_owner_wire::HarnessContextOwnerDraftOperation::RemoveObjective
            )
        })
    {
        values.push("workflow:context:objective:edit");
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

pub(super) fn grant_snapshot(grant: &AdmittedSubjectGrant) -> GrantSnapshot {
    GrantSnapshot {
        grant_id: grant.grant_id.clone(),
        issuer: grant.issuer.clone(),
        subject: grant.subject.clone(),
        permission: grant.permission.as_str().to_owned(),
        project_id: grant.scope.project_id.clone(),
        run_id: grant.scope.run_id.clone(),
        episode_id: grant.scope.episode_id.clone(),
        agent_id: grant.scope.agent_id.clone(),
        not_before: grant.not_before,
        expires_at: grant.expires_at,
        revocation_generation: grant.revocation_generation,
    }
}

pub(super) fn response_from_lookup(
    original: &ContextOwnerInvocationV2,
    lookup: HarnessResponseV1,
) -> Result<Option<HarnessResponseV1>, AdmissionError> {
    match (&original.operation, lookup) {
        (
            ContextOwnerOperationV2::CreateDraft { .. }
            | ContextOwnerOperationV2::PatchDraft { .. }
            | ContextOwnerOperationV2::CreatePreview { .. },
            HarnessResponseV1::MutationLookup(Some(receipt)),
        ) => Ok(Some(HarnessResponseV1::MutationReceipt(receipt))),
        (
            ContextOwnerOperationV2::PublishDraft { .. },
            HarnessResponseV1::PublicationLookup(Some(receipt)),
        ) => Ok(Some(HarnessResponseV1::PublicationReceipt(receipt))),
        (
            ContextOwnerOperationV2::SubmitControl { .. },
            value @ HarnessResponseV1::ControlReceipt(_),
        ) => Ok(Some(value)),
        (
            ContextOwnerOperationV2::CreateDraft { .. }
            | ContextOwnerOperationV2::PatchDraft { .. }
            | ContextOwnerOperationV2::CreatePreview { .. },
            HarnessResponseV1::MutationLookup(None),
        )
        | (
            ContextOwnerOperationV2::PublishDraft { .. },
            HarnessResponseV1::PublicationLookup(None),
        ) => Ok(None),
        _ => Err(AdmissionError::Denied),
    }
}

pub(super) fn required_permission_names(
    operation: &ContextOwnerOperationV2,
    use_kind: AdmissionUse,
) -> Result<Vec<String>, AdmissionError> {
    Ok(required_console_permissions(operation, use_kind)?
        .into_iter()
        .map(|permission| permission.as_str().to_owned())
        .collect())
}
