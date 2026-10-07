// SPDX-License-Identifier: MIT

//! Authenticated, subject-bound request admission for an operator-provided Console server.
//!
//! The module accepts a verifier port rather than choosing a token format or issuer. The verifier
//! returns bounded claims; this module checks them against operator configuration, reserves exact
//! subject grants, resolves a per-request Harness reference, then creates the only trusted
//! `FacadeRequest` constructor. The raw bearer is borrowed only for verification and is never
//! retained in the returned request or grant store.

use crate::control::Scope;
use crate::harness_facade::{FacadePermission, FacadeRequest, HarnessFacadeConfig};
use crate::http::HttpRequest;
use crate::protected_owner_credentials::{
    ProtectedOwnerCredentialResolver, ResolvedOwnerCredential, harness_scope_for,
};
use crate::subject_grants::{AdmittedSubjectGrant, SubjectGrantStore};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

mod owner_invocation;
mod validation;
pub(crate) use owner_invocation::AuthenticatedOwnerInvocation;
use validation::{
    bearer_from_headers, map_credential_error, map_grant_error, unique_harness_scopes,
    valid_claim_text, validate_admitted_grants, validate_http_envelope, validate_owner_credential,
};

/// Maximum syntax length for configured issuer and audience identifiers.
pub const MAX_PRINCIPAL_CLAIM_BYTES: usize = 128;

/// Closed, non-secret claims returned by an operator-configured verifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedPrincipalClaims {
    pub issuer: String,
    pub subject: String,
    pub audience: String,
    pub credential_id: String,
    pub expires_at: u64,
}

/// Cryptographic or operator-specific bearer verifier.
///
/// Implementations must not log or retain the borrowed bearer. Returning claims is an assertion
/// made by this configured trust boundary; ingress still checks their syntax and configured
/// issuer/audience/expiry before creating a principal.
pub trait PrincipalVerifier {
    fn verify(
        &mut self,
        bearer: &[u8],
    ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrincipalVerificationError {
    Invalid,
    Unavailable,
}

impl std::fmt::Display for PrincipalVerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "bearer authentication failed",
            Self::Unavailable => "principal verifier is unavailable",
        })
    }
}

impl std::error::Error for PrincipalVerificationError {}

/// Identity produced only after the configured verifier's claims pass ingress validation.
///
/// There is no public constructor, `Clone`, serde implementation, or bearer field.
pub struct VerifiedPrincipal {
    issuer: String,
    subject: String,
    audience: String,
    credential_id: String,
    expires_at: u64,
}

impl VerifiedPrincipal {
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn audience(&self) -> &str {
        &self.audience
    }

    pub fn credential_id(&self) -> &str {
        &self.credential_id
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Operator-controlled issuer/audience expectations for an ingress instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthenticatedIngressConfig {
    issuer: String,
    audience: String,
}

impl AuthenticatedIngressConfig {
    pub fn new(
        issuer: impl Into<String>,
        audience: impl Into<String>,
    ) -> Result<Self, AuthenticatedIngressError> {
        let config = Self {
            issuer: issuer.into(),
            audience: audience.into(),
        };
        if !valid_claim_text(&config.issuer) || !valid_claim_text(&config.audience) {
            return Err(AuthenticatedIngressError::InvalidConfiguration);
        }
        Ok(config)
    }
}

/// Errors expose only stable local classifications, never verifier, SQLite or credential data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticatedIngressError {
    InvalidConfiguration,
    InvalidHttpRequest,
    InvalidBearer,
    Unauthorized,
    GrantDenied,
    VerifierUnavailable,
    GrantStoreUnavailable,
    OwnerCredentialUnavailable,
    OwnerCredentialDenied,
    InvalidOwnerCredential,
}

impl std::fmt::Display for AuthenticatedIngressError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidConfiguration => "authenticated ingress configuration is invalid",
            Self::InvalidHttpRequest => "trusted request is invalid",
            Self::InvalidBearer => "bearer authentication failed",
            Self::Unauthorized => "request is not authorized",
            Self::GrantDenied => "subject grant is unavailable",
            Self::VerifierUnavailable => "principal verifier is unavailable",
            Self::GrantStoreUnavailable => "subject grant store is unavailable",
            Self::OwnerCredentialUnavailable => "owner credential is unavailable",
            Self::OwnerCredentialDenied => "owner credential is not authorized",
            Self::InvalidOwnerCredential => "owner credential metadata is invalid",
        })
    }
}

impl std::error::Error for AuthenticatedIngressError {}

/// An authenticated Console principal together with one operation-scoped owner reference.
///
/// This value contains no bearer. It is crate-constructed by `AuthenticatedIngress` and consumed
/// by `FacadeRequest`; fields stay private so callers cannot forge trusted authorization.
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct TrustedRequestAuthorization {
    issuer: String,
    subject: String,
    principal_expires_at: u64,
    grants: Vec<AdmittedSubjectGrant>,
    owner_credential: ResolvedOwnerCredential,
    request_identity: std::sync::Arc<()>,
    admitted_until: Instant,
}

impl TrustedRequestAuthorization {
    pub(crate) fn subject(&self) -> &str {
        &self.subject
    }

    pub(crate) fn permits(&self, permission: FacadePermission, scope: &Scope, now: u64) -> bool {
        Instant::now() < self.admitted_until
            && self.principal_expires_at > now
            && self.owner_credential.descriptor().expires_at() > now
            && self.owner_credential.descriptor().scope() == scope
            && self.grants.iter().any(|grant| {
                grant.issuer == self.issuer
                    && grant.subject == self.subject
                    && grant.scope == *scope
                    && grant.permission == permission
                    && grant.not_before <= now
                    && grant.expires_at > now
                    && self
                        .owner_credential
                        .descriptor()
                        .permits_exact_scope(harness_scope_for(permission))
            })
    }

    pub(crate) fn owner_reference(&self) -> &crate::harness_facade::ProtectedAuthReference {
        self.owner_credential.reference()
    }

    pub(crate) fn request_identity(&self) -> &std::sync::Arc<()> {
        &self.request_identity
    }

    pub(crate) fn permission_names(&self, scope: &Scope, now: u64) -> Vec<String> {
        if Instant::now() >= self.admitted_until {
            return Vec::new();
        }
        self.grants
            .iter()
            .filter(|grant| {
                grant.scope == *scope
                    && grant.not_before <= now
                    && grant.expires_at > now
                    && self
                        .owner_credential
                        .descriptor()
                        .permits_exact_scope(harness_scope_for(grant.permission))
            })
            .map(|grant| grant.permission.as_str().to_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// Trusted bearer-to-grant-to-owner admission. Each call resolves a new owner reference; no
/// principal, grant result or credential reference is cached between requests.
pub struct AuthenticatedIngress<V, G> {
    config: AuthenticatedIngressConfig,
    verifier: V,
    grants: G,
}

impl<V, G> AuthenticatedIngress<V, G>
where
    V: PrincipalVerifier,
    G: SubjectGrantStore,
{
    pub fn new(config: AuthenticatedIngressConfig, verifier: V, grants: G) -> Self {
        Self {
            config,
            verifier,
            grants,
        }
    }

    pub fn grants(&self) -> &G {
        &self.grants
    }

    pub fn grants_mut(&mut self) -> &mut G {
        &mut self.grants
    }

    /// Verify one HTTP request and create a trusted facade request.
    ///
    /// The caller supplies required and optional Console permissions after selecting its closed
    /// operation route. The facade independently enforces each permission before dispatch.
    pub fn admit_http<R: ProtectedOwnerCredentialResolver>(
        &mut self,
        request: &HttpRequest,
        facade: &HarnessFacadeConfig,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
        resolver: &mut R,
    ) -> Result<FacadeRequest, AuthenticatedIngressError> {
        let admission_started = Instant::now();
        facade
            .validate()
            .map_err(|_| AuthenticatedIngressError::InvalidConfiguration)?;
        validate_http_envelope(request, facade, required)?;
        let bearer = bearer_from_headers(request)?;
        let claims = self.verifier.verify(bearer).map_err(|error| match error {
            PrincipalVerificationError::Invalid => AuthenticatedIngressError::InvalidBearer,
            PrincipalVerificationError::Unavailable => {
                AuthenticatedIngressError::VerifierUnavailable
            }
        })?;
        let principal = self.validate_claims(claims, now)?;
        let admitted = self
            .grants
            .reserve(&principal, &facade.scope, required, optional, now)
            .map_err(map_grant_error)?;
        validate_admitted_grants(
            &admitted,
            &principal,
            &facade.scope,
            required,
            optional,
            now,
        )?;

        let requested_scopes = unique_harness_scopes(&admitted);
        let resolved = resolver
            .resolve(&principal, &facade.scope, &requested_scopes, now)
            .map_err(map_credential_error)?;
        validate_owner_credential(&resolved, &principal, &facade.scope, required, now)?;
        let grants = admitted
            .into_iter()
            .filter(|grant| {
                resolved
                    .descriptor()
                    .permits_exact_scope(harness_scope_for(grant.permission))
            })
            .collect::<Vec<_>>();
        let authority_expires_at = grants
            .iter()
            .map(|grant| grant.expires_at)
            .chain([principal.expires_at, resolved.descriptor().expires_at()])
            .min()
            .ok_or(AuthenticatedIngressError::GrantDenied)?;
        let lifetime = authority_expires_at
            .checked_sub(now)
            .filter(|remaining| *remaining > 0)
            .ok_or(AuthenticatedIngressError::Unauthorized)?;
        let admitted_until = admission_started
            .checked_add(Duration::from_secs(lifetime))
            .ok_or(AuthenticatedIngressError::Unauthorized)?;
        if Instant::now() >= admitted_until {
            return Err(AuthenticatedIngressError::Unauthorized);
        }
        let auth = TrustedRequestAuthorization {
            issuer: principal.issuer.clone(),
            subject: principal.subject.clone(),
            principal_expires_at: principal.expires_at,
            grants,
            owner_credential: resolved,
            request_identity: std::sync::Arc::new(()),
            admitted_until,
        };
        let host = request.header("host").unwrap_or_default().to_owned();
        let origin = request.header("origin").map(str::to_owned);
        let csrf = request
            .header("x-csrf-token")
            .map(|value| value.as_bytes().to_vec());
        Ok(FacadeRequest::new_trusted(host, origin, csrf, now, auth))
    }

    fn validate_claims(
        &self,
        claims: VerifiedPrincipalClaims,
        now: u64,
    ) -> Result<VerifiedPrincipal, AuthenticatedIngressError> {
        if claims.issuer != self.config.issuer
            || claims.audience != self.config.audience
            || claims.expires_at <= now
            || !valid_claim_text(&claims.issuer)
            || !crate::harness_facade::valid_id(&claims.subject)
            || !valid_claim_text(&claims.audience)
            || !valid_claim_text(&claims.credential_id)
        {
            return Err(AuthenticatedIngressError::Unauthorized);
        }
        Ok(VerifiedPrincipal {
            issuer: claims.issuer,
            subject: claims.subject,
            audience: claims.audience,
            credential_id: claims.credential_id,
            expires_at: claims.expires_at,
        })
    }
}
