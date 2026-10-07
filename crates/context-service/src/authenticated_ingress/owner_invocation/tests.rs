// SPDX-License-Identifier: MIT

use super::super::{
    AuthenticatedIngress, AuthenticatedIngressConfig, AuthenticatedIngressError,
    PrincipalVerificationError, PrincipalVerifier, VerifiedPrincipalClaims,
};
use super::*;
use crate::control::Scope;
use crate::harness_context_owner_wire::{
    ConsoleOwnerIdentityV2, ConsoleOwnerScopeV2, ContextOwnerInvocationV2, ContextOwnerOperationV2,
    HarnessActorIdentityV2, OwnerIdentityCorrelationV2,
};
use crate::harness_facade::{FacadePermission, ProtectedAuthReference, RetentionPolicy};
use crate::http::HttpRequest;
use crate::protected_owner_credentials::{
    CredentialResolutionError, OwnerCredentialDescriptor, ProtectedOwnerCredentialResolver,
    ResolvedOwnerCredential,
};
use crate::subject_grants::{AdmittedSubjectGrant, SubjectGrantError, SubjectGrantStore};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::Duration;

const ISSUER: &str = "https://console.example/issuer";
const AUDIENCE: &str = "context-console-test";
const SUBJECT: &str = "console-user";
const GRANT_ID: &str = "grant-current-association";
const OWNER_REFERENCE: &str = "protected-owner-reference";

const GRANT_UNCHANGED: u8 = 0;
const GRANT_REVOKED: u8 = 1;
const GRANT_GENERATION_CHANGED: u8 = 2;
const GRANT_REPLACED: u8 = 3;

struct FixedVerifier {
    claims: VerifiedPrincipalClaims,
    delay: Duration,
    calls: usize,
}

impl PrincipalVerifier for FixedVerifier {
    fn verify(
        &mut self,
        bearer: &[u8],
    ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError> {
        self.calls += 1;
        if !self.delay.is_zero() {
            thread::sleep(self.delay);
        }
        if bearer == b"synthetic-bearer" {
            Ok(self.claims.clone())
        } else {
            Err(PrincipalVerificationError::Invalid)
        }
    }
}

struct FixedGrantStore {
    omit_grants: bool,
    first_expires_at: u64,
    expiry_offset_seconds: Option<u64>,
    required: Vec<FacadePermission>,
    optional: Vec<FacadePermission>,
    mutation: Arc<AtomicU8>,
    delay: Duration,
    calls: usize,
}

impl SubjectGrantStore for FixedGrantStore {
    fn reserve(
        &mut self,
        principal: &super::super::VerifiedPrincipal,
        scope: &Scope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
    ) -> Result<Vec<AdmittedSubjectGrant>, SubjectGrantError> {
        self.calls += 1;
        self.required = required.to_vec();
        self.optional = optional.to_vec();
        if !self.delay.is_zero() {
            thread::sleep(self.delay);
        }
        if self.omit_grants {
            return Ok(Vec::new());
        }
        let mutation = self.mutation.load(Ordering::SeqCst);
        if mutation == GRANT_REVOKED {
            return Ok(Vec::new());
        }
        Ok(required
            .iter()
            .enumerate()
            .map(|(index, permission)| AdmittedSubjectGrant {
                grant_id: if mutation == GRANT_REPLACED && index == 0 {
                    "replacement-grant".to_owned()
                } else if index == 0 {
                    GRANT_ID.to_owned()
                } else {
                    format!("grant-extra-{index}")
                },
                issuer: principal.issuer().to_owned(),
                subject: principal.subject().to_owned(),
                permission: *permission,
                scope: scope.clone(),
                not_before: 0,
                expires_at: self
                    .expiry_offset_seconds
                    .and_then(|offset| now.checked_add(offset))
                    .unwrap_or(self.first_expires_at),
                revocation_generation: if mutation == GRANT_GENERATION_CHANGED {
                    2
                } else {
                    1
                },
            })
            .collect())
    }
}

struct FixedResolver {
    expires_at: u64,
    delay: Duration,
    mutation: Option<(Arc<AtomicU8>, u8)>,
    calls: usize,
    requested_scopes: Vec<String>,
}

impl ProtectedOwnerCredentialResolver for FixedResolver {
    fn resolve(
        &mut self,
        principal: &super::super::VerifiedPrincipal,
        scope: &Scope,
        requested_harness_scopes: &[&'static str],
        _now: u64,
    ) -> Result<ResolvedOwnerCredential, CredentialResolutionError> {
        self.calls += 1;
        self.requested_scopes = requested_harness_scopes
            .iter()
            .map(|scope| (*scope).to_owned())
            .collect();
        if !self.delay.is_zero() {
            thread::sleep(self.delay);
        }
        if let Some((mutation, value)) = &self.mutation {
            mutation.store(*value, Ordering::SeqCst);
        }
        let descriptor = OwnerCredentialDescriptor::new(
            principal.issuer(),
            principal.subject(),
            principal.subject(),
            scope.clone(),
            requested_harness_scopes
                .iter()
                .map(|scope| (*scope).to_owned()),
            self.expires_at,
        );
        let reference = ProtectedAuthReference::new(OWNER_REFERENCE)
            .map_err(|_| CredentialResolutionError::Invalid)?;
        Ok(ResolvedOwnerCredential::new(reference, descriptor))
    }
}

fn scope() -> Scope {
    Scope {
        project_id: "project-a".to_owned(),
        run_id: "run-a".to_owned(),
        episode_id: "episode-a".to_owned(),
        agent_id: "agent-a".to_owned(),
    }
}

fn invocation(grant_expires_at: u64, credential_expires_at: u64) -> ContextOwnerInvocationV2 {
    let scope = scope();
    ContextOwnerInvocationV2 {
        schema_version: "ascension.console.context-owner-invocation.v2".to_owned(),
        identity: OwnerIdentityCorrelationV2 {
            console: ConsoleOwnerIdentityV2 {
                issuer: ISSUER.to_owned(),
                subject: SUBJECT.to_owned(),
                audience: AUDIENCE.to_owned(),
                credential_id: "console-credential-a".to_owned(),
                grant_id: GRANT_ID.to_owned(),
                grant_generation: 1,
                grant_expires_at,
            },
            console_scope: ConsoleOwnerScopeV2 {
                project_id: scope.project_id,
                run_id: scope.run_id.clone(),
                episode_id: scope.episode_id,
                agent_id: scope.agent_id,
            },
            harness: HarnessActorIdentityV2 {
                actor_subject: SUBJECT.to_owned(),
                owner_id: "harness-owner-a".to_owned(),
                workflow_run_id: scope.run_id.clone(),
                credential_reference_id: OWNER_REFERENCE.to_owned(),
                credential_expires_at,
            },
        },
        expected_binding: None,
        operation: ContextOwnerOperationV2::CurrentAssociation {
            workflow_run_id: scope.run_id,
        },
    }
}

fn request() -> HttpRequest {
    HttpRequest {
        method: "GET".to_owned(),
        target: "/v2/runs/run-a/context-control/state".to_owned(),
        headers: vec![
            ("host".to_owned(), "console.example".to_owned()),
            ("origin".to_owned(), "https://console.example".to_owned()),
            (
                "authorization".to_owned(),
                "Bearer synthetic-bearer".to_owned(),
            ),
        ],
        body: Vec::new(),
    }
}

fn facade(scope: Scope) -> HarnessFacadeConfig {
    HarnessFacadeConfig::new(
        scope,
        "console.example",
        Some("https://console.example".to_owned()),
        None,
        RetentionPolicy::default(),
    )
    .expect("valid facade config")
}

fn ingress(
    omit_grants: bool,
    claims_expiry: u64,
    grant_expiry: u64,
) -> AuthenticatedIngress<FixedVerifier, FixedGrantStore> {
    AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new(ISSUER, AUDIENCE).expect("valid ingress config"),
        FixedVerifier {
            claims: VerifiedPrincipalClaims {
                issuer: ISSUER.to_owned(),
                subject: SUBJECT.to_owned(),
                audience: AUDIENCE.to_owned(),
                credential_id: "console-credential-a".to_owned(),
                expires_at: claims_expiry,
            },
            delay: Duration::ZERO,
            calls: 0,
        },
        FixedGrantStore {
            omit_grants,
            first_expires_at: grant_expiry,
            expiry_offset_seconds: None,
            required: Vec::new(),
            optional: Vec::new(),
            mutation: Arc::new(AtomicU8::new(GRANT_UNCHANGED)),
            delay: Duration::ZERO,
            calls: 0,
        },
    )
}

fn resolver(expires_at: u64, delay: Duration) -> FixedResolver {
    FixedResolver {
        expires_at,
        delay,
        mutation: None,
        calls: 0,
        requested_scopes: Vec::new(),
    }
}

fn now() -> u64 {
    trusted_unix_seconds().expect("system clock is after Unix epoch")
}

#[test]
fn typed_invocation_admission_uses_operation_policy_and_revalidates_exact_grants() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut ingress = ingress(false, now + 500, now + 300);
    let mutation = Arc::clone(&ingress.grants.mutation);
    let mut resolver = resolver(now + 400, Duration::ZERO);
    let admitted = ingress
        .admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        )
        .expect("verified read admission with post-resolution grant refresh");

    assert_eq!(ingress.verifier.calls, 1);
    assert_eq!(ingress.grants.calls, 2);
    assert_eq!(
        ingress.grants.required,
        vec![FacadePermission::MetadataRead]
    );
    assert!(ingress.grants.optional.is_empty());
    assert_eq!(resolver.calls, 1);
    assert_eq!(resolver.requested_scopes, vec!["workflow:read"]);
    assert_eq!(
        admitted.credential().reference().as_str(),
        OWNER_REFERENCE,
        "the exact resolved credential is carried with the admission"
    );
    assert!(
        admitted
            .admission()
            .validate_for(&invocation, AdmissionUse::ReadOnly, now)
            .is_ok()
    );
    assert!(
        admitted
            .admission()
            .validate_for(&invocation, AdmissionUse::Write, now)
            .is_err()
    );

    ingress
        .revalidate_owner_invocation(&admitted)
        .expect("unchanged active grant can be refreshed");
    assert_eq!(ingress.grants.calls, 3);
    mutation.store(GRANT_REVOKED, Ordering::SeqCst);
    assert!(matches!(
        ingress.revalidate_owner_invocation(&admitted),
        Err(AuthenticatedIngressError::GrantDenied)
    ));
    assert_eq!(ingress.grants.calls, 4);
}

#[test]
fn resolver_time_revocation_prevents_sealing_a_stale_admission() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut ingress = ingress(false, now + 500, now + 300);
    let mutation = Arc::clone(&ingress.grants.mutation);
    let mut resolver = resolver(now + 400, Duration::ZERO);
    resolver.mutation = Some((mutation, GRANT_REVOKED));

    assert!(matches!(
        ingress.admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::GrantDenied)
    ));
    assert_eq!(ingress.grants.calls, 2);
    assert_eq!(resolver.calls, 1);
}

#[test]
fn changed_grant_generation_or_replacement_is_refused() {
    for mutation_value in [GRANT_GENERATION_CHANGED, GRANT_REPLACED] {
        let now = now();
        let invocation = invocation(now + 300, now + 400);
        let mut ingress = ingress(false, now + 500, now + 300);
        let mutation = Arc::clone(&ingress.grants.mutation);
        let mut resolver = resolver(now + 400, Duration::ZERO);
        resolver.mutation = Some((mutation, mutation_value));

        assert!(matches!(
            ingress.admit_owner_invocation(
                &request(),
                &facade(scope()),
                &invocation,
                AdmissionUse::ReadOnly,
                &mut resolver,
            ),
            Err(AuthenticatedIngressError::GrantDenied)
        ));
        assert_eq!(ingress.grants.calls, 2);
        assert_eq!(resolver.calls, 1);
    }
}

#[test]
fn principal_expiry_during_verifier_work_is_rejected_after_verification() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut ingress = ingress(false, now + 2, now + 300);
    ingress.verifier.delay = Duration::from_millis(2_200);
    let mut resolver = resolver(now + 400, Duration::ZERO);

    assert!(matches!(
        ingress.admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::Unauthorized)
    ));
    assert_eq!(ingress.verifier.calls, 1);
    assert_eq!(ingress.grants.calls, 0);
    assert_eq!(resolver.calls, 0);
}

#[test]
fn grant_expiry_during_resolver_work_is_rejected_before_sealing() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut ingress = ingress(false, now + 500, now + 300);
    ingress.grants.expiry_offset_seconds = Some(2);
    let mut resolver = resolver(now + 400, Duration::from_millis(2_200));

    assert!(matches!(
        ingress.admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::GrantDenied)
    ));
    assert_eq!(ingress.grants.calls, 1);
    assert_eq!(resolver.calls, 1);
}

#[test]
fn grant_expiry_during_store_wait_is_rejected_before_owner_resolution() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut ingress = ingress(false, now + 500, now + 300);
    ingress.grants.expiry_offset_seconds = Some(2);
    ingress.grants.delay = Duration::from_millis(2_200);
    let mut resolver = resolver(now + 400, Duration::ZERO);

    assert!(matches!(
        ingress.admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::GrantDenied)
    ));
    assert_eq!(ingress.grants.calls, 1);
    assert_eq!(resolver.calls, 0);
}

#[test]
fn mismatched_route_scope_and_missing_grant_stop_before_owner_resolution() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut current_ingress = ingress(false, now + 500, now + 300);
    let mut resolver = resolver(now + 400, Duration::ZERO);
    let mut other_scope = scope();
    other_scope.run_id = "run-other".to_owned();
    assert!(matches!(
        current_ingress.admit_owner_invocation(
            &request(),
            &facade(other_scope),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::Unauthorized)
    ));
    assert_eq!(current_ingress.verifier.calls, 0);
    assert_eq!(current_ingress.grants.calls, 0);
    assert_eq!(resolver.calls, 0);

    let mut missing = ingress(true, now + 500, now + 300);
    assert!(matches!(
        missing.admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::GrantDenied)
    ));
    assert_eq!(missing.verifier.calls, 1);
    assert_eq!(missing.grants.calls, 1);
    assert_eq!(resolver.calls, 0);
}

#[test]
fn unsupported_use_kind_is_rejected_before_bearer_verification() {
    let now = now();
    let invocation = invocation(now + 300, now + 400);
    let mut ingress = ingress(false, now + 500, now + 300);
    let mut resolver = resolver(now + 400, Duration::ZERO);
    assert!(matches!(
        ingress.admit_owner_invocation(
            &request(),
            &facade(scope()),
            &invocation,
            AdmissionUse::Write,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::InvalidHttpRequest)
    ));
    assert_eq!(ingress.verifier.calls, 0);
    assert_eq!(ingress.grants.calls, 0);
    assert_eq!(resolver.calls, 0);
}

#[test]
fn legacy_http_admission_keeps_its_existing_explicit_time_path() {
    let now = now();
    let mut ingress = ingress(false, now + 500, now + 300);
    let mut resolver = resolver(now + 400, Duration::ZERO);
    let _admitted = ingress
        .admit_http(
            &request(),
            &facade(scope()),
            &[FacadePermission::MetadataRead],
            &[],
            now,
            &mut resolver,
        )
        .expect("legacy admission remains usable");

    assert_eq!(ingress.verifier.calls, 1);
    assert_eq!(ingress.grants.calls, 1);
    assert_eq!(resolver.calls, 1);
}
