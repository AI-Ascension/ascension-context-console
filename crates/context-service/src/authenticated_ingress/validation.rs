// SPDX-License-Identifier: MIT

use super::{AuthenticatedIngressError, VerifiedPrincipal};
use crate::control::Scope;
use crate::harness_facade::{
    FacadePermission, HarnessFacadeConfig, MAX_FACADE_BODY_BYTES, MAX_FACADE_HOST_BYTES,
    MAX_FACADE_ORIGIN_BYTES, MAX_FACADE_TOKEN_BYTES, SecretDigest,
};
use crate::http::HttpRequest;
use crate::protected_owner_credentials::{
    CredentialResolutionError, ResolvedOwnerCredential, harness_scope_for,
};
use crate::subject_grants::{AdmittedSubjectGrant, SubjectGrantError};
use std::collections::BTreeSet;

pub(super) fn validate_http_envelope(
    request: &HttpRequest,
    facade: &HarnessFacadeConfig,
    required: &[FacadePermission],
) -> Result<(), AuthenticatedIngressError> {
    validate_http_envelope_with_body_limit(request, facade, required, MAX_FACADE_BODY_BYTES)
}

pub(super) fn validate_owner_http_envelope(
    request: &HttpRequest,
    facade: &HarnessFacadeConfig,
    required: &[FacadePermission],
) -> Result<(), AuthenticatedIngressError> {
    validate_http_envelope_with_body_limit(request, facade, required, 1024 * 1024)
}

fn validate_http_envelope_with_body_limit(
    request: &HttpRequest,
    facade: &HarnessFacadeConfig,
    required: &[FacadePermission],
    body_limit: usize,
) -> Result<(), AuthenticatedIngressError> {
    if request.body.len() > body_limit
        || request.headers.len() > 32
        || request.target.len() > 4096
        || request.target.contains("://")
        || request.target.contains('%')
        || request.target.contains('\\')
        || request.target.contains('\0')
        || (request.method != "GET" && request.method != "POST")
        || (request.method == "GET" && !request.body.is_empty())
        || request.header("host") != Some(facade.expected_host.as_str())
        || facade.expected_origin.as_deref() != request.header("origin")
    {
        return Err(AuthenticatedIngressError::InvalidHttpRequest);
    }
    if request.headers.iter().any(|(name, value)| {
        name.is_empty()
            || name.len() > 64
            || value.len() > 2048
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || value.chars().any(char::is_control)
    }) || request
        .header("host")
        .is_some_and(|host| host.len() > MAX_FACADE_HOST_BYTES)
        || request
            .header("origin")
            .is_some_and(|origin| origin.len() > MAX_FACADE_ORIGIN_BYTES)
    {
        return Err(AuthenticatedIngressError::InvalidHttpRequest);
    }
    if [
        "authorization",
        "host",
        "origin",
        "x-csrf-token",
        "x-principal",
    ]
    .iter()
    .any(|name| header_count(request, name) > 1)
        || header_count(request, "x-principal") != 0
    {
        return Err(AuthenticatedIngressError::InvalidHttpRequest);
    }
    let write = required.iter().any(|permission| {
        !matches!(
            permission,
            FacadePermission::MetadataRead | FacadePermission::ContentRead
        )
    });
    if write {
        let digest: SecretDigest = facade
            .csrf_digest
            .ok_or(AuthenticatedIngressError::Unauthorized)?;
        let csrf = request
            .header("x-csrf-token")
            .ok_or(AuthenticatedIngressError::Unauthorized)?;
        if !digest.matches(csrf.as_bytes()) || csrf.len() > MAX_FACADE_TOKEN_BYTES {
            return Err(AuthenticatedIngressError::Unauthorized);
        }
    }
    Ok(())
}

pub(super) fn bearer_from_headers(
    request: &HttpRequest,
) -> Result<&[u8], AuthenticatedIngressError> {
    let value = request
        .header("authorization")
        .ok_or(AuthenticatedIngressError::InvalidBearer)?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or(AuthenticatedIngressError::InvalidBearer)?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.is_empty()
        || token.len() > MAX_FACADE_TOKEN_BYTES
        || token
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(AuthenticatedIngressError::InvalidBearer);
    }
    Ok(token.as_bytes())
}

fn header_count(request: &HttpRequest, name: &str) -> usize {
    request
        .headers
        .iter()
        .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
        .count()
}

pub(super) fn valid_claim_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= super::MAX_PRINCIPAL_CLAIM_BYTES
        && !value.contains('\0')
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub(super) fn validate_admitted_grants(
    grants: &[AdmittedSubjectGrant],
    principal: &VerifiedPrincipal,
    scope: &Scope,
    required: &[FacadePermission],
    optional: &[FacadePermission],
    now: u64,
) -> Result<(), AuthenticatedIngressError> {
    if required.is_empty() || !unique_permissions(required, optional) {
        return Err(AuthenticatedIngressError::InvalidHttpRequest);
    }
    let mut seen = BTreeSet::new();
    for grant in grants {
        if grant.issuer != principal.issuer
            || grant.subject != principal.subject
            || grant.scope != *scope
            || grant.expires_at <= now
            || grant.not_before > now
            || (!required.contains(&grant.permission) && !optional.contains(&grant.permission))
            || !seen.insert(grant.permission)
        {
            return Err(AuthenticatedIngressError::GrantDenied);
        }
    }
    if required.iter().all(|permission| seen.contains(permission)) {
        Ok(())
    } else {
        Err(AuthenticatedIngressError::GrantDenied)
    }
}

fn unique_permissions(required: &[FacadePermission], optional: &[FacadePermission]) -> bool {
    let mut seen = BTreeSet::new();
    required
        .iter()
        .chain(optional.iter())
        .all(|permission| seen.insert(*permission))
}

pub(super) fn unique_harness_scopes(grants: &[AdmittedSubjectGrant]) -> Vec<&'static str> {
    grants
        .iter()
        .map(|grant| harness_scope_for(grant.permission))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn validate_owner_credential(
    credential: &ResolvedOwnerCredential,
    principal: &VerifiedPrincipal,
    scope: &Scope,
    required: &[FacadePermission],
    now: u64,
) -> Result<(), AuthenticatedIngressError> {
    let descriptor = credential.descriptor();
    if !descriptor.is_valid() {
        return Err(AuthenticatedIngressError::InvalidOwnerCredential);
    }
    if descriptor.console_issuer() != principal.issuer
        || descriptor.console_subject() != principal.subject
        || descriptor.harness_subject() != principal.subject
        || descriptor.scope() != scope
        || descriptor.expires_at() <= now
        || descriptor.expires_at() > principal.expires_at
        || !required
            .iter()
            .all(|permission| descriptor.permits_exact_scope(harness_scope_for(*permission)))
    {
        return Err(AuthenticatedIngressError::OwnerCredentialDenied);
    }
    Ok(())
}

pub(super) fn map_grant_error(error: SubjectGrantError) -> AuthenticatedIngressError {
    match error {
        SubjectGrantError::Invalid => AuthenticatedIngressError::GrantDenied,
        SubjectGrantError::Duplicate
        | SubjectGrantError::Capacity
        | SubjectGrantError::Denied
        | SubjectGrantError::NotFound => AuthenticatedIngressError::GrantDenied,
        SubjectGrantError::Corrupt
        | SubjectGrantError::StorageLimit
        | SubjectGrantError::StoreUnavailable => AuthenticatedIngressError::GrantStoreUnavailable,
    }
}

pub(super) fn map_credential_error(error: CredentialResolutionError) -> AuthenticatedIngressError {
    match error {
        CredentialResolutionError::Unavailable => {
            AuthenticatedIngressError::OwnerCredentialUnavailable
        }
        CredentialResolutionError::Denied => AuthenticatedIngressError::OwnerCredentialDenied,
        CredentialResolutionError::Invalid => AuthenticatedIngressError::InvalidOwnerCredential,
    }
}
