use crate::harness_context_owner_wire::{ContextOwnerInvocationV2, MAX_HARNESS_JSON_BODY_BYTES};

use super::StoreError;
use super::record::{
    AdmissionError, AdmissionUse, EntryState, InvocationRecord, TrustedInvocationAdmission,
};
use super::storage::MAX_CIPHERTEXT_BYTES;
use zeroize::Zeroizing;
pub(super) fn push_attempt(
    record: &mut InvocationRecord,
    attempt: super::record::AttemptSnapshot,
) -> Result<(), StoreError> {
    if record.attempts.len() >= super::record::MAX_ATTEMPTS {
        return Err(StoreError::AttemptLimit);
    }
    record.attempts.push(attempt);
    Ok(())
}

pub(super) fn advance(
    record: &mut InvocationRecord,
    next: EntryState,
    now: u64,
) -> Result<(), StoreError> {
    let allowed = matches!(
        (record.state, next),
        (EntryState::Prepared, EntryState::WriteClaimed)
            | (
                EntryState::WriteClaimed,
                EntryState::Completed | EntryState::Unknown
            )
            | (
                EntryState::WriteClaimed | EntryState::Unknown | EntryState::LookupClaimed,
                EntryState::LookupClaimed
            )
            | (
                EntryState::LookupClaimed,
                EntryState::Completed | EntryState::Unknown
            )
    );
    if !allowed {
        return Err(StoreError::RecoveryRequired);
    }
    record.sequence = record
        .sequence
        .checked_add(1)
        .ok_or(StoreError::StorageLimit)?;
    record.state = next;
    record.updated_at = now.max(record.created_at);
    Ok(())
}

pub(super) fn ensure_response_headroom(record: &InvocationRecord) -> Result<(), StoreError> {
    let current =
        Zeroizing::new(serde_json::to_vec(record).map_err(|_| StoreError::StoreUnavailable)?);
    if current
        .len()
        .checked_add(MAX_HARNESS_JSON_BODY_BYTES)
        .and_then(|size| size.checked_add(4096))
        .is_none_or(|size| size > MAX_CIPHERTEXT_BYTES)
    {
        return Err(StoreError::StorageLimit);
    }
    Ok(())
}

pub(super) fn origin_matches_invocation(
    origin: &super::record::AdmissionSnapshot,
    invocation: &ContextOwnerInvocationV2,
) -> bool {
    let identity = &invocation.identity;
    origin.issuer == identity.console.issuer
        && origin.subject == identity.console.subject
        && origin.audience == identity.console.audience
        && origin.console_credential_id == identity.console.credential_id
        && origin.grants.iter().any(|grant| {
            grant.grant_id == identity.console.grant_id
                && grant.revocation_generation == identity.console.grant_generation
                && grant.expires_at == identity.console.grant_expires_at
        })
        && origin.scope.project_id == identity.console_scope.project_id
        && origin.scope.run_id == identity.console_scope.run_id
        && origin.scope.episode_id == identity.console_scope.episode_id
        && origin.scope.agent_id == identity.console_scope.agent_id
        && origin.harness_actor == identity.harness.actor_subject
        && origin.harness_owner == identity.harness.owner_id
        && origin.harness_workflow_run == identity.harness.workflow_run_id
        && origin.binding == invocation.expected_binding
        && origin.credential_reference_id == identity.harness.credential_reference_id
        && origin.credential_expires_at == identity.harness.credential_expires_at
}

pub(super) fn map_admission_error(error: AdmissionError) -> StoreError {
    match error {
        AdmissionError::Denied => StoreError::Denied,
        AdmissionError::UnsupportedOperation => StoreError::UnsupportedOperation,
    }
}

pub(super) fn validate_admission_current(
    admission: &TrustedInvocationAdmission,
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    minimum_now: u64,
) -> Result<u64, StoreError> {
    let wall_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Denied)?
        .as_secs();
    let now = minimum_now.max(wall_now);
    admission
        .validate_for(invocation, use_kind, now)
        .map_err(map_admission_error)?;
    Ok(now)
}
