use crate::harness_context_owner_wire::{ContextOwnerInvocationV2, MAX_HARNESS_JSON_BODY_BYTES};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

use super::StoreError;
use super::crypto::{OwnerInvocationKeyMaterial, decrypt, encrypt, lookup_tag};
use super::engine_helpers::{map_admission_error, origin_matches_invocation};
use super::operations::{
    admission_matches, required_harness_scopes, required_permission_names, same_call,
    stable_locator,
};
use super::record::{EntryState, InvocationRecord, TrustedInvocationAdmission};
use super::storage::{MAX_CIPHERTEXT_BYTES, StoredRow};
pub(super) fn open_record_with_keys(
    keys: &OwnerInvocationKeyMaterial,
    row: &StoredRow,
) -> Result<InvocationRecord, StoreError> {
    let plaintext = decrypt(keys, keys.index_key_id(), row)?;
    let record: InvocationRecord =
        serde_json::from_slice(&plaintext).map_err(|_| StoreError::StoreCorrupt)?;
    if record.state != row.state
        || record.sequence != row.sequence
        || record.data_key_id != row.data_key_id
    {
        return Err(StoreError::StoreCorrupt);
    }
    validate_record(&record)?;
    let expected_tag = lookup_tag(
        keys.index_key(),
        &stable_locator(&record.invocation).map_err(map_admission_error)?,
    )?;
    if expected_tag != row.tag {
        return Err(StoreError::StoreCorrupt);
    }
    Ok(record)
}

pub(super) fn seal_record_with_keys(
    keys: &OwnerInvocationKeyMaterial,
    record: &mut InvocationRecord,
    tag: [u8; 32],
    entry_id: [u8; 16],
) -> Result<StoredRow, StoreError> {
    record.data_key_id = keys.current_data_key_id().to_owned();
    validate_record(record)?;
    let plaintext =
        Zeroizing::new(serde_json::to_vec(record).map_err(|_| StoreError::StoreUnavailable)?);
    if plaintext.len() > MAX_CIPHERTEXT_BYTES {
        return Err(StoreError::StorageLimit);
    }
    let (nonce, ciphertext) = encrypt(
        keys,
        &tag,
        &entry_id,
        record.state as i64,
        record.sequence,
        &plaintext,
    )?;
    if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
        return Err(StoreError::StorageLimit);
    }
    Ok(StoredRow {
        tag,
        entry_id,
        state: record.state,
        sequence: record.sequence,
        data_key_id: record.data_key_id.clone(),
        nonce,
        ciphertext,
    })
}

pub(super) fn check_exact_match(
    record: &InvocationRecord,
    invocation: &ContextOwnerInvocationV2,
    admission: &TrustedInvocationAdmission,
) -> Result<(), StoreError> {
    if record.schema_version != super::record::RECORD_SCHEMA_VERSION
        || !same_call(&record.invocation, invocation)
        || !same_call(invocation, &admission.invocation)
        || !admission_matches(&record.origin, &admission.snapshot)
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub(super) fn validate_record(record: &InvocationRecord) -> Result<(), StoreError> {
    if record.schema_version != super::record::RECORD_SCHEMA_VERSION
        || record.sequence == 0
        || record.attempts.len() > super::record::MAX_ATTEMPTS
        || record.origin.grants.is_empty()
        || record.origin.grants.len() > super::record::MAX_AUTH_GRANTS
        || record.origin.required_permissions.is_empty()
        || record.created_at == 0
        || record.updated_at < record.created_at
        || !valid_state_history(record)
        || record.state != EntryState::Completed && record.response.is_some()
        || record.state == EntryState::Completed && record.response.is_none()
    {
        return Err(StoreError::StoreCorrupt);
    }
    record
        .invocation
        .validate()
        .map_err(|_| StoreError::StoreCorrupt)?;
    let endpoint = record
        .invocation
        .endpoint()
        .map_err(|_| StoreError::StoreCorrupt)?;
    let body = record
        .invocation
        .harness_body()
        .map_err(|_| StoreError::StoreCorrupt)?
        .ok_or(StoreError::StoreCorrupt)?;
    let expected_permissions = required_permission_names(
        &record.invocation.operation,
        super::record::AdmissionUse::Write,
    )
    .map_err(|_| StoreError::StoreCorrupt)?;
    let expected_harness_scopes = required_harness_scopes(
        &record.invocation.operation,
        super::record::AdmissionUse::Write,
    )
    .map_err(|_| StoreError::StoreCorrupt)?
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if endpoint != record.endpoint
        || body.as_slice() != record.canonical_body.as_bytes()
        || body.len() > MAX_HARNESS_JSON_BODY_BYTES
        || !origin_matches_invocation(&record.origin, &record.invocation)
        || record.origin.required_permissions != expected_permissions
        || record.origin.harness_scopes != expected_harness_scopes
        || record.origin.principal_expires_at <= record.created_at
        || record.origin.credential_expires_at <= record.created_at
        || !valid_grant_snapshots(
            &record.origin.grants,
            &record.origin.scope,
            &record.origin.issuer,
            &record.origin.subject,
            &record.origin.required_permissions,
        )
    {
        return Err(StoreError::StoreCorrupt);
    }
    if let Some(response) = &record.response {
        let tagged =
            Zeroizing::new(serde_json::to_vec(response).map_err(|_| StoreError::StoreCorrupt)?);
        // The journal retains the closed tagged response enum, whereas the owner
        // route sends its raw payload. Revalidate that payload against the original
        // operation and compare the resulting variant as well as all receipt fields.
        let payload = Zeroizing::new(
            match response {
                crate::harness_context_owner_wire::HarnessResponseV1::MutationReceipt(value) => {
                    serde_json::to_vec(value)
                }
                crate::harness_context_owner_wire::HarnessResponseV1::PublicationReceipt(value) => {
                    serde_json::to_vec(value)
                }
                crate::harness_context_owner_wire::HarnessResponseV1::SourcePublication(value) => {
                    serde_json::to_vec(value)
                }
                crate::harness_context_owner_wire::HarnessResponseV1::ControlReceipt(value) => {
                    serde_json::to_vec(value)
                }
                _ => return Err(StoreError::StoreCorrupt),
            }
            .map_err(|_| StoreError::StoreCorrupt)?,
        );
        if tagged.len() > MAX_HARNESS_JSON_BODY_BYTES
            || payload.len() > MAX_HARNESS_JSON_BODY_BYTES
            || record
                .invocation
                .decode_response(&payload)
                .map_err(|_| StoreError::StoreCorrupt)?
                != *response
        {
            return Err(StoreError::StoreCorrupt);
        }
    }
    for attempt in &record.attempts {
        let use_kind = match attempt.kind {
            super::record::AttemptKind::Write => super::record::AdmissionUse::Write,
            super::record::AttemptKind::Lookup => super::record::AdmissionUse::ExactLookup,
        };
        let expected = required_permission_names(&record.invocation.operation, use_kind)
            .map_err(|_| StoreError::StoreCorrupt)?;
        let expected_harness_scopes =
            required_harness_scopes(&record.invocation.operation, use_kind)
                .map_err(|_| StoreError::StoreCorrupt)?
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>();
        if attempt.grants.is_empty()
            || attempt.grants.len() > super::record::MAX_AUTH_GRANTS
            || attempt.console_credential_id.is_empty()
            || attempt.principal_expires_at == 0
            || attempt.required_permissions != expected
            || attempt.harness_scopes != expected_harness_scopes
            || attempt.credential_reference_id.is_empty()
            || attempt.credential_expires_at == 0
            || attempt.credential_expires_at > attempt.principal_expires_at
            || !valid_grant_snapshots(
                &attempt.grants,
                &record.origin.scope,
                &record.origin.issuer,
                &record.origin.subject,
                &attempt.required_permissions,
            )
        {
            return Err(StoreError::StoreCorrupt);
        }
    }
    Ok(())
}

fn valid_grant_snapshots(
    grants: &[super::record::GrantSnapshot],
    scope: &crate::control::Scope,
    issuer: &str,
    subject: &str,
    required_permissions: &[String],
) -> bool {
    let mut seen = BTreeSet::new();
    grants.iter().all(|grant| {
        !grant.grant_id.is_empty()
            && grant.issuer == issuer
            && grant.subject == subject
            && grant.issuer.len() <= crate::authenticated_ingress::MAX_PRINCIPAL_CLAIM_BYTES
            && grant.subject.len() <= crate::authenticated_ingress::MAX_PRINCIPAL_CLAIM_BYTES
            && !grant.issuer.is_empty()
            && !grant.subject.is_empty()
            && grant.permission.len() <= 64
            && seen.insert(grant.permission.as_str())
            && grant.project_id == scope.project_id
            && grant.run_id == scope.run_id
            && grant.episode_id == scope.episode_id
            && grant.agent_id == scope.agent_id
            && grant.expires_at > grant.not_before
    }) && required_permissions
        .iter()
        .all(|permission| seen.contains(permission.as_str()))
}

fn valid_state_history(record: &InvocationRecord) -> bool {
    use super::record::AttemptKind;

    let last_kind = record.attempts.last().map(|attempt| attempt.kind);
    match record.state {
        EntryState::Prepared => record.attempts.is_empty(),
        EntryState::WriteClaimed | EntryState::DefinitelyNotSent => {
            last_kind == Some(AttemptKind::Write)
        }
        EntryState::LookupClaimed => last_kind == Some(AttemptKind::Lookup),
        EntryState::Unknown | EntryState::Completed => {
            matches!(last_kind, Some(AttemptKind::Write | AttemptKind::Lookup))
        }
    }
}
