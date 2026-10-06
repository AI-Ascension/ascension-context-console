// SPDX-License-Identifier: MIT

use context_service::{
    AuthenticatedIngress, AuthenticatedIngressConfig, ControlScope, CredentialResolutionError,
    HarnessFacadeConfig, HttpRequest, OwnerCredentialDescriptor, PrincipalVerificationError,
    PrincipalVerifier, ProtectedAuthReference, ProtectedOwnerCredentialResolver,
    ResolvedOwnerCredential, RetentionPolicy, SqliteSubjectGrantStore, VerifiedPrincipal,
    VerifiedPrincipalClaims,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub(super) const ISSUER: &str = "https://console.example/issuer";
pub(super) const AUDIENCE: &str = "context-console-test";
pub(super) const SUBJECT: &str = "console-user";
pub(super) const BEARER: &str = "synthetic-console-bearer";

static NEXT_DATABASE: AtomicU64 = AtomicU64::new(0);

pub(super) struct TestDatabase {
    directory: PathBuf,
    pub(super) path: PathBuf,
}

impl TestDatabase {
    pub(super) fn new() -> Self {
        let unique = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "console-verified-ingress-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create private test directory");
        Self {
            path: directory.join("grants.sqlite"),
            directory,
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(super) fn scope() -> ControlScope {
    ControlScope::new("project-a", "run-a", "episode-a", "agent-a").expect("valid scope")
}

pub(super) fn claims(expires_at: u64) -> VerifiedPrincipalClaims {
    VerifiedPrincipalClaims {
        issuer: ISSUER.to_owned(),
        subject: SUBJECT.to_owned(),
        audience: AUDIENCE.to_owned(),
        credential_id: "console-credential-a".to_owned(),
        expires_at,
    }
}

pub(super) fn facade(scope: ControlScope) -> HarnessFacadeConfig {
    HarnessFacadeConfig::new(
        scope,
        "console.example",
        Some("https://console.example".to_owned()),
        None,
        RetentionPolicy::default(),
    )
    .expect("valid facade config")
}

pub(super) fn request(scope: &ControlScope) -> HttpRequest {
    HttpRequest {
        method: "GET".to_owned(),
        target: format!("/v2/runs/{}/context-control/state", scope.run_id),
        headers: vec![
            ("host".to_owned(), "console.example".to_owned()),
            ("origin".to_owned(), "https://console.example".to_owned()),
            ("authorization".to_owned(), format!("Bearer {BEARER}")),
        ],
        body: Vec::new(),
    }
}

#[derive(Clone)]
pub(super) struct FixedVerifier {
    claims: VerifiedPrincipalClaims,
    calls: Arc<AtomicUsize>,
}

impl PrincipalVerifier for FixedVerifier {
    fn verify(
        &mut self,
        bearer: &[u8],
    ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if bearer == BEARER.as_bytes() {
            Ok(self.claims.clone())
        } else {
            Err(PrincipalVerificationError::Invalid)
        }
    }
}

pub(super) struct FixedResolver {
    pub(super) calls: Arc<AtomicUsize>,
    pub(super) blocked: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl ProtectedOwnerCredentialResolver for FixedResolver {
    fn resolve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &ControlScope,
        requested_scopes: &[&'static str],
        now: u64,
    ) -> Result<ResolvedOwnerCredential, CredentialResolutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((entered, proceed)) = self.blocked.take() {
            entered
                .send(())
                .map_err(|_| CredentialResolutionError::Unavailable)?;
            proceed
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| CredentialResolutionError::Unavailable)?;
        }
        let descriptor = OwnerCredentialDescriptor::new(
            principal.issuer(),
            principal.subject(),
            principal.subject(),
            scope.clone(),
            requested_scopes.iter().map(|value| (*value).to_owned()),
            principal.expires_at().max(now.saturating_add(1)),
        );
        let reference = ProtectedAuthReference::new("synthetic-owner-reference")
            .map_err(|_| CredentialResolutionError::Invalid)?;
        Ok(ResolvedOwnerCredential::new(reference, descriptor))
    }
}

pub(super) struct UnavailableResolver {
    pub(super) calls: Arc<AtomicUsize>,
}

impl ProtectedOwnerCredentialResolver for UnavailableResolver {
    fn resolve(
        &mut self,
        _principal: &VerifiedPrincipal,
        _scope: &ControlScope,
        _requested_scopes: &[&'static str],
        _now: u64,
    ) -> Result<ResolvedOwnerCredential, CredentialResolutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(CredentialResolutionError::Unavailable)
    }
}

pub(super) fn ingress(
    database: &TestDatabase,
    principal: VerifiedPrincipalClaims,
    verifier_calls: Arc<AtomicUsize>,
) -> AuthenticatedIngress<FixedVerifier, SqliteSubjectGrantStore> {
    let grants = SqliteSubjectGrantStore::open(&database.path).expect("open grant store");
    AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new(ISSUER, AUDIENCE).expect("ingress config"),
        FixedVerifier {
            claims: principal,
            calls: verifier_calls,
        },
        grants,
    )
}
