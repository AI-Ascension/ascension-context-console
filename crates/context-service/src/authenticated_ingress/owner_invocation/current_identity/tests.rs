// SPDX-License-Identifier: MIT

use crate::authenticated_ingress::{
    AuthenticatedIngress, AuthenticatedIngressConfig, AuthenticatedIngressError,
    AuthenticatedOwnerInvocation, PrincipalVerificationError, PrincipalVerifier, VerifiedPrincipal,
    VerifiedPrincipalClaims,
};
use crate::control::Scope;
use crate::harness_context_owner_wire::{
    ConsoleOwnerIdentityV2, ConsoleOwnerScopeV2, ContextOwnerOperationV2, HarnessActorIdentityV2,
    OwnerIdentityCorrelationV2,
};
use crate::harness_facade::{
    FacadePermission, HarnessFacadeConfig, ProtectedAuthReference, RetentionPolicy,
};
use crate::http::HttpRequest;
use crate::owner_invocation_store::AdmissionUse;
use crate::protected_owner_credentials::{
    CredentialResolutionError, OwnerCredentialDescriptor, ProtectedOwnerCredentialResolver,
    ResolvedOwnerCredential,
};
use crate::subject_grants::{AdmittedSubjectGrant, SubjectGrantError, SubjectGrantStore};
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ISSUER: &str = "https://console.example/issuer";
const AUDIENCE: &str = "context-console-test";
const SUBJECT: &str = "console-user";
const CONSOLE_CREDENTIAL_ID: &str = "console-credential-a";
const GRANT_ID: &str = "grant-current-association";
const OWNER_REFERENCE: &str = "protected-owner-reference";

struct FixedVerifier {
    claims: VerifiedPrincipalClaims,
    reject: bool,
    calls: usize,
}

impl PrincipalVerifier for FixedVerifier {
    fn verify(
        &mut self,
        bearer: &[u8],
    ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError> {
        self.calls += 1;
        if self.reject || bearer != b"synthetic-bearer" {
            return Err(PrincipalVerificationError::Invalid);
        }
        Ok(self.claims.clone())
    }
}

struct FixedGrantStore {
    expires_at: u64,
    revoked: Arc<AtomicBool>,
    calls: usize,
}

impl SubjectGrantStore for FixedGrantStore {
    fn reserve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
    ) -> Result<Vec<AdmittedSubjectGrant>, SubjectGrantError> {
        self.calls += 1;
        if !optional.is_empty() || self.revoked.load(Ordering::SeqCst) || self.expires_at <= now {
            return Ok(Vec::new());
        }
        Ok(required
            .iter()
            .map(|permission| AdmittedSubjectGrant {
                grant_id: GRANT_ID.to_owned(),
                issuer: principal.issuer().to_owned(),
                subject: principal.subject().to_owned(),
                permission: *permission,
                scope: scope.clone(),
                not_before: 0,
                expires_at: self.expires_at,
                revocation_generation: 1,
            })
            .collect())
    }
}

struct FixedResolver {
    reference: String,
    expires_at: u64,
    delay: Duration,
    revoke_grants: Option<Arc<AtomicBool>>,
    calls: usize,
    requested_scopes: Vec<String>,
}

impl ProtectedOwnerCredentialResolver for FixedResolver {
    fn resolve(
        &mut self,
        principal: &VerifiedPrincipal,
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
        if let Some(revoked) = &self.revoke_grants {
            revoked.store(true, Ordering::SeqCst);
        }
        let reference = ProtectedAuthReference::new(self.reference.clone())
            .map_err(|_| CredentialResolutionError::Invalid)?;
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
        Ok(ResolvedOwnerCredential::new(reference, descriptor))
    }
}

struct Fixture {
    ingress: AuthenticatedIngress<FixedVerifier, FixedGrantStore>,
    resolver: FixedResolver,
    admitted: AuthenticatedOwnerInvocation,
    invocation: ContextOwnerInvocationV2,
    facade: HarnessFacadeConfig,
    request: HttpRequest,
    expires_at: u64,
}

fn fixture(lifetime_seconds: u64) -> Result<Fixture, Box<dyn Error>> {
    let expires_at = expiration_after(lifetime_seconds)?;
    let scope = scope();
    let invocation = ContextOwnerInvocationV2 {
        schema_version: "ascension.console.context-owner-invocation.v2".to_owned(),
        identity: OwnerIdentityCorrelationV2 {
            console: ConsoleOwnerIdentityV2 {
                issuer: ISSUER.to_owned(),
                subject: SUBJECT.to_owned(),
                audience: AUDIENCE.to_owned(),
                credential_id: CONSOLE_CREDENTIAL_ID.to_owned(),
                grant_id: GRANT_ID.to_owned(),
                grant_generation: 1,
                grant_expires_at: expires_at,
            },
            console_scope: ConsoleOwnerScopeV2 {
                project_id: scope.project_id.clone(),
                run_id: scope.run_id.clone(),
                episode_id: scope.episode_id.clone(),
                agent_id: scope.agent_id.clone(),
            },
            harness: HarnessActorIdentityV2 {
                actor_subject: SUBJECT.to_owned(),
                owner_id: "harness-owner-a".to_owned(),
                workflow_run_id: scope.run_id.clone(),
                credential_reference_id: OWNER_REFERENCE.to_owned(),
                credential_expires_at: expires_at,
            },
        },
        expected_binding: None,
        operation: ContextOwnerOperationV2::CurrentAssociation {
            workflow_run_id: scope.run_id.clone(),
        },
    };
    let facade = HarnessFacadeConfig::new(
        scope,
        "console.example",
        Some("https://console.example".to_owned()),
        None,
        RetentionPolicy::default(),
    )?;
    let request = request(&invocation)?;
    let revoked = Arc::new(AtomicBool::new(false));
    let mut ingress = AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new(ISSUER, AUDIENCE)?,
        FixedVerifier {
            claims: VerifiedPrincipalClaims {
                issuer: ISSUER.to_owned(),
                subject: SUBJECT.to_owned(),
                audience: AUDIENCE.to_owned(),
                credential_id: CONSOLE_CREDENTIAL_ID.to_owned(),
                expires_at,
            },
            reject: false,
            calls: 0,
        },
        FixedGrantStore {
            expires_at,
            revoked,
            calls: 0,
        },
    );
    let mut resolver = FixedResolver {
        reference: OWNER_REFERENCE.to_owned(),
        expires_at,
        delay: Duration::ZERO,
        revoke_grants: None,
        calls: 0,
        requested_scopes: Vec::new(),
    };
    let admitted = ingress.admit_owner_invocation(
        &request,
        &facade,
        &invocation,
        AdmissionUse::ReadOnly,
        &mut resolver,
    )?;
    Ok(Fixture {
        ingress,
        resolver,
        admitted,
        invocation,
        facade,
        request,
        expires_at,
    })
}

fn request(invocation: &ContextOwnerInvocationV2) -> Result<HttpRequest, serde_json::Error> {
    Ok(HttpRequest {
        method: "POST".to_owned(),
        target: "/v2/context-owner/invocations".to_owned(),
        headers: vec![
            ("host".to_owned(), "console.example".to_owned()),
            ("origin".to_owned(), "https://console.example".to_owned()),
            (
                "authorization".to_owned(),
                "Bearer synthetic-bearer".to_owned(),
            ),
        ],
        body: serde_json::to_vec(invocation)?,
    })
}

fn scope() -> Scope {
    Scope {
        project_id: "project-a".to_owned(),
        run_id: "run-a".to_owned(),
        episode_id: "episode-a".to_owned(),
        agent_id: "agent-a".to_owned(),
    }
}

fn expiration_after(seconds: u64) -> Result<u64, Box<dyn Error>> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    let rounded = elapsed
        .as_secs()
        .checked_add(u64::from(elapsed.subsec_nanos() != 0))
        .ok_or_else(|| std::io::Error::other("test clock overflow"))?;
    rounded
        .checked_add(seconds)
        .ok_or_else(|| std::io::Error::other("test expiry overflow").into())
}

fn unix_seconds() -> Result<u64, Box<dyn Error>> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH)?;
    elapsed
        .as_secs()
        .checked_add(u64::from(elapsed.subsec_nanos() != 0))
        .ok_or_else(|| std::io::Error::other("test clock overflow").into())
}

#[test]
fn unchanged_live_identity_and_slot_keep_the_original_pair_and_deadline()
-> Result<(), Box<dyn Error>> {
    let mut fixture = fixture(300)?;
    let before = fixture.admitted.admission().deadline_for(
        &fixture.invocation,
        AdmissionUse::ReadOnly,
        unix_seconds()?,
    )?;
    fixture.ingress.revalidate_current_owner_identity(
        &fixture.request,
        &fixture.facade,
        &fixture.admitted,
        &mut fixture.resolver,
    )?;
    let after = fixture.admitted.admission().deadline_for(
        &fixture.invocation,
        AdmissionUse::ReadOnly,
        unix_seconds()?,
    )?;

    assert_eq!(before, after);
    assert_eq!(fixture.ingress.verifier.calls, 2);
    assert_eq!(fixture.ingress.grants.calls, 3);
    assert_eq!(fixture.resolver.calls, 2);
    assert_eq!(fixture.resolver.requested_scopes, vec!["workflow:read"]);
    assert_eq!(
        fixture.admitted.credential().reference().as_str(),
        OWNER_REFERENCE
    );
    Ok(())
}

#[test]
fn current_verifier_rejection_and_changed_principal_are_refused_before_resolution()
-> Result<(), Box<dyn Error>> {
    let mut rejected = fixture(300)?;
    rejected.ingress.verifier.reject = true;
    assert!(matches!(
        rejected.ingress.revalidate_current_owner_identity(
            &rejected.request,
            &rejected.facade,
            &rejected.admitted,
            &mut rejected.resolver,
        ),
        Err(AuthenticatedIngressError::InvalidBearer)
    ));
    assert_eq!(rejected.resolver.calls, 1);

    for change_expiry in [false, true] {
        let mut changed = fixture(300)?;
        if change_expiry {
            changed.ingress.verifier.claims.expires_at = changed.expires_at + 1;
        } else {
            changed.ingress.verifier.claims.credential_id = "replacement-credential".to_owned();
        }
        assert!(matches!(
            changed.ingress.revalidate_current_owner_identity(
                &changed.request,
                &changed.facade,
                &changed.admitted,
                &mut changed.resolver,
            ),
            Err(AuthenticatedIngressError::Unauthorized)
        ));
        assert_eq!(changed.resolver.calls, 1);
    }
    Ok(())
}

#[test]
fn changed_reference_or_descriptor_is_not_substituted() -> Result<(), Box<dyn Error>> {
    let mut reference_changed = fixture(300)?;
    reference_changed.resolver.reference = "replacement-owner-reference".to_owned();
    assert!(matches!(
        reference_changed.ingress.revalidate_current_owner_identity(
            &reference_changed.request,
            &reference_changed.facade,
            &reference_changed.admitted,
            &mut reference_changed.resolver,
        ),
        Err(AuthenticatedIngressError::OwnerCredentialDenied)
    ));

    let mut descriptor_changed = fixture(300)?;
    descriptor_changed.resolver.expires_at -= 1;
    assert!(matches!(
        descriptor_changed
            .ingress
            .revalidate_current_owner_identity(
                &descriptor_changed.request,
                &descriptor_changed.facade,
                &descriptor_changed.admitted,
                &mut descriptor_changed.resolver,
            ),
        Err(AuthenticatedIngressError::OwnerCredentialDenied)
    ));
    Ok(())
}

#[test]
fn grant_revoked_during_resolution_is_refused_after_the_wait() -> Result<(), Box<dyn Error>> {
    let mut fixture = fixture(300)?;
    fixture.resolver.revoke_grants = Some(Arc::clone(&fixture.ingress.grants.revoked));
    fixture.resolver.delay = Duration::from_millis(10);
    assert!(matches!(
        fixture.ingress.revalidate_current_owner_identity(
            &fixture.request,
            &fixture.facade,
            &fixture.admitted,
            &mut fixture.resolver,
        ),
        Err(AuthenticatedIngressError::GrantDenied)
    ));
    assert_eq!(fixture.resolver.calls, 2);
    assert_eq!(fixture.ingress.grants.calls, 3);
    Ok(())
}

#[test]
fn slow_resolution_cannot_extend_the_original_admission_deadline() -> Result<(), Box<dyn Error>> {
    let mut fixture = fixture(3)?;
    let deadline = fixture.admitted.admission().deadline_for(
        &fixture.invocation,
        AdmissionUse::ReadOnly,
        unix_seconds()?,
    )?;
    fixture.resolver.delay =
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(100);
    let result = fixture.ingress.revalidate_current_owner_identity(
        &fixture.request,
        &fixture.facade,
        &fixture.admitted,
        &mut fixture.resolver,
    );

    assert!(matches!(
        result,
        Err(AuthenticatedIngressError::GrantDenied)
            | Err(AuthenticatedIngressError::OwnerCredentialDenied)
    ));
    assert_eq!(fixture.resolver.calls, 2);
    assert!(
        fixture
            .admitted
            .admission()
            .deadline_for(&fixture.invocation, AdmissionUse::ReadOnly, unix_seconds()?)
            .is_err()
    );
    Ok(())
}

#[test]
fn changed_body_or_facade_scope_stops_before_current_verification() -> Result<(), Box<dyn Error>> {
    let mut body_changed = fixture(300)?;
    let mut other = body_changed.invocation.clone();
    other.identity.console.credential_id = "other-console-credential".to_owned();
    let body_request = request(&other)?;
    assert!(matches!(
        body_changed.ingress.revalidate_current_owner_identity(
            &body_request,
            &body_changed.facade,
            &body_changed.admitted,
            &mut body_changed.resolver,
        ),
        Err(AuthenticatedIngressError::InvalidHttpRequest)
    ));
    assert_eq!(body_changed.ingress.verifier.calls, 1);
    assert_eq!(body_changed.resolver.calls, 1);

    let mut scope_changed = fixture(300)?;
    let mut other_scope = scope();
    other_scope.run_id = "run-other".to_owned();
    let facade = HarnessFacadeConfig::new(
        other_scope,
        "console.example",
        Some("https://console.example".to_owned()),
        None,
        RetentionPolicy::default(),
    )?;
    assert!(matches!(
        scope_changed.ingress.revalidate_current_owner_identity(
            &scope_changed.request,
            &facade,
            &scope_changed.admitted,
            &mut scope_changed.resolver,
        ),
        Err(AuthenticatedIngressError::Unauthorized)
    ));
    assert_eq!(scope_changed.ingress.verifier.calls, 1);
    assert_eq!(scope_changed.resolver.calls, 1);
    Ok(())
}
