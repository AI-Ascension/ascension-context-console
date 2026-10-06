// SPDX-License-Identifier: MIT

use context_service::{
    AuthenticatedIngress, AuthenticatedIngressConfig, AuthenticatedIngressError, ControlScope,
    CredentialResolutionError, FacadePermission, FacadeRequest, HarnessFacadeConfig, HttpRequest,
    OwnerCredentialDescriptor, PrincipalVerificationError, PrincipalVerifier,
    ProtectedAuthReference, ProtectedOwnerCredentialResolver, ResolvedOwnerCredential,
    RetentionPolicy, SqliteSubjectGrantStore, SubjectGrantSpec, VerifiedPrincipal,
    VerifiedPrincipalClaims,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

static NEXT_DATABASE: AtomicU64 = AtomicU64::new(0);

pub struct TestDatabase {
    directory: PathBuf,
    path: PathBuf,
}

impl TestDatabase {
    pub fn new() -> Self {
        let unique = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "console-subject-grants-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create private test directory");
        let path = directory.join("grants.sqlite");
        Self { directory, path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub fn scope() -> ControlScope {
    ControlScope::new("project-a", "run-a", "episode-a", "agent-a").expect("valid test scope")
}

pub fn claims(issuer: &str, subject: &str, expires_at: u64) -> VerifiedPrincipalClaims {
    VerifiedPrincipalClaims {
        issuer: issuer.to_owned(),
        subject: subject.to_owned(),
        audience: "context-console-test".to_owned(),
        credential_id: "console-credential-a".to_owned(),
        expires_at,
    }
}

pub fn grant_spec(
    grant_id: &str,
    issuer: &str,
    subject: &str,
    permission: FacadePermission,
    scope: &ControlScope,
    not_before: u64,
    expires_at: u64,
) -> SubjectGrantSpec {
    SubjectGrantSpec {
        grant_id: grant_id.to_owned(),
        issuer: issuer.to_owned(),
        subject: subject.to_owned(),
        permission,
        scope: scope.clone(),
        not_before,
        expires_at,
    }
}

pub fn facade(scope: ControlScope) -> HarnessFacadeConfig {
    HarnessFacadeConfig::new(
        scope,
        "console.example",
        Some("https://console.example".to_owned()),
        None,
        RetentionPolicy::default(),
    )
    .expect("valid facade configuration")
}

pub fn http_request(_scope: &ControlScope) -> HttpRequest {
    HttpRequest {
        method: "GET".to_owned(),
        target: "/context".to_owned(),
        headers: vec![
            ("host".to_owned(), "console.example".to_owned()),
            ("origin".to_owned(), "https://console.example".to_owned()),
            (
                "authorization".to_owned(),
                "Bearer synthetic-console-bearer".to_owned(),
            ),
        ],
        body: Vec::new(),
    }
}

#[derive(Clone)]
pub struct FixedVerifier(pub VerifiedPrincipalClaims);

impl PrincipalVerifier for FixedVerifier {
    fn verify(
        &mut self,
        bearer: &[u8],
    ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError> {
        if bearer == b"synthetic-console-bearer" {
            Ok(self.0.clone())
        } else {
            Err(PrincipalVerificationError::Invalid)
        }
    }
}

pub struct CredentialResolver {
    pub calls: Arc<AtomicUsize>,
    pub blocked: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

impl ProtectedOwnerCredentialResolver for CredentialResolver {
    fn resolve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &ControlScope,
        requested_harness_scopes: &[&'static str],
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
            requested_harness_scopes
                .iter()
                .map(|value| (*value).to_owned()),
            principal.expires_at().max(now.saturating_add(1)),
        );
        let reference = ProtectedAuthReference::new("synthetic-owner-reference")
            .map_err(|_| CredentialResolutionError::Invalid)?;
        Ok(ResolvedOwnerCredential::new(reference, descriptor))
    }
}

pub fn admit(
    path: &Path,
    principal: VerifiedPrincipalClaims,
    requested_scope: ControlScope,
    permission: FacadePermission,
    now: u64,
    calls: Arc<AtomicUsize>,
) -> Result<FacadeRequest, AuthenticatedIngressError> {
    let store = SqliteSubjectGrantStore::open(path)
        .map_err(|_| AuthenticatedIngressError::GrantStoreUnavailable)?;
    let mut ingress = AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new(principal.issuer.clone(), principal.audience.clone())?,
        FixedVerifier(principal),
        store,
    );
    let mut resolver = CredentialResolver {
        calls,
        blocked: None,
    };
    ingress.admit_http(
        &http_request(&requested_scope),
        &facade(requested_scope),
        &[permission],
        &[],
        now,
        &mut resolver,
    )
}
