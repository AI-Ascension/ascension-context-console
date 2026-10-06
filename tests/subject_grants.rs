// SPDX-License-Identifier: MIT

//! Durable grant reservation, revocation and reopen behavior through the production ingress.

#[path = "subject_grants/support.rs"]
mod support;

use context_service::{
    AuthenticatedIngress, AuthenticatedIngressConfig, AuthenticatedIngressError, FacadePermission,
    HarnessBackedContextService, SqliteSubjectGrantStore, SubjectGrantError, SubjectGrantStore,
    VerifiedPrincipal, VerifiedPrincipalClaims,
};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use support::{TestDatabase, claims, facade, grant_spec, http_request, scope};

#[test]
fn grants_survive_reopen_and_remain_issuer_subject_scope_permission_and_time_bound() {
    let database = TestDatabase::new();
    let grant_scope = scope();
    let mut store = SqliteSubjectGrantStore::open(database.path()).expect("open store");
    store
        .provision(&grant_spec(
            "grant-reader",
            "https://issuer.example",
            "alice",
            FacadePermission::MetadataRead,
            &grant_scope,
            100,
            200,
        ))
        .expect("provision read grant");
    drop(store);

    let principal_claims = claims("https://issuer.example", "alice", 900);
    let accepted = support::admit(
        database.path(),
        principal_claims.clone(),
        grant_scope.clone(),
        FacadePermission::MetadataRead,
        100,
        Arc::new(AtomicUsize::new(0)),
    );
    assert!(accepted.is_ok(), "not_before is inclusive after reopen");

    assert_eq!(
        support::admit(
            database.path(),
            principal_claims.clone(),
            grant_scope.clone(),
            FacadePermission::MetadataRead,
            200,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "expiry is exclusive"
    );
    assert_eq!(
        support::admit(
            database.path(),
            claims("https://issuer.example", "bob", 900),
            grant_scope.clone(),
            FacadePermission::MetadataRead,
            150,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "another Console subject cannot use the grant"
    );
    assert_eq!(
        support::admit(
            database.path(),
            claims("https://other-issuer.example", "alice", 900),
            grant_scope.clone(),
            FacadePermission::MetadataRead,
            150,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "the same subject string from another issuer cannot use the grant"
    );
    let mut other_scope = grant_scope.clone();
    other_scope.run_id.push_str("-other");
    assert_eq!(
        support::admit(
            database.path(),
            principal_claims.clone(),
            other_scope,
            FacadePermission::MetadataRead,
            150,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "the full run scope is part of the grant"
    );
    assert_eq!(
        support::admit(
            database.path(),
            principal_claims,
            grant_scope,
            FacadePermission::ContentRead,
            150,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "a read grant does not grant content access"
    );
}

#[test]
fn revocation_persists_across_reopen_and_repeating_it_keeps_generation() {
    let database = TestDatabase::new();
    let grant_scope = scope();
    let mut store = SqliteSubjectGrantStore::open(database.path()).expect("open store");
    store
        .provision(&grant_spec(
            "grant-revoke",
            "https://issuer.example",
            "alice",
            FacadePermission::MetadataRead,
            &grant_scope,
            10,
            900,
        ))
        .expect("provision grant");
    drop(store);

    let mut reopened = SqliteSubjectGrantStore::open(database.path()).expect("reopen store");
    assert_eq!(reopened.revoke("grant-revoke"), Ok(1));
    assert_eq!(reopened.revoke("grant-revoke"), Ok(1));
    drop(reopened);

    assert_eq!(
        support::admit(
            database.path(),
            claims("https://issuer.example", "alice", 900),
            grant_scope,
            FacadePermission::MetadataRead,
            100,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "revocation remains effective after reopening the database"
    );
}

#[test]
fn independent_handles_serialize_a_reservation_against_revocation() {
    let database = TestDatabase::new();
    let grant_scope = scope();
    let mut provisioner = SqliteSubjectGrantStore::open(database.path()).expect("open store");
    provisioner
        .provision(&grant_spec(
            "grant-race",
            "https://issuer.example",
            "alice",
            FacadePermission::MetadataRead,
            &grant_scope,
            10,
            900,
        ))
        .expect("provision grant");
    drop(provisioner);

    let gate = Arc::new(Barrier::new(3));
    let mut ingress = make_ingress(
        database.path(),
        claims("https://issuer.example", "alice", 900),
        Some(gate.clone()),
    );
    let request = http_request(&grant_scope);
    let facade_config = facade(grant_scope.clone());
    let worker = std::thread::spawn(move || {
        let mut resolver = support::CredentialResolver {
            calls: Arc::new(AtomicUsize::new(0)),
            blocked: None,
        };
        let result = ingress.admit_http(
            &request,
            &facade_config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        );
        (result, resolver.calls.load(Ordering::SeqCst))
    });

    let mut revoker = SqliteSubjectGrantStore::open(database.path()).expect("second store handle");
    let revoker_gate = gate.clone();
    let revocation = std::thread::spawn(move || {
        revoker_gate.wait();
        revoker.revoke("grant-race")
    });
    gate.wait();

    let (admission, resolver_calls) = worker.join().expect("admission worker");
    assert_eq!(revocation.join().expect("revocation worker"), Ok(1));
    match admission {
        Ok(_) => assert_eq!(resolver_calls, 1, "reservation won before revocation"),
        Err(AuthenticatedIngressError::GrantDenied) => {
            assert_eq!(resolver_calls, 0, "revocation won before reservation")
        }
        Err(error) => panic!("unexpected racing admission result: {error:?}"),
    }

    assert_eq!(
        support::admit(
            database.path(),
            claims("https://issuer.example", "alice", 900),
            grant_scope,
            FacadePermission::MetadataRead,
            100,
            Arc::new(AtomicUsize::new(0)),
        ),
        Err(AuthenticatedIngressError::GrantDenied),
        "a fresh reservation observes the completed revocation"
    );
}

#[test]
fn revocation_after_reservation_does_not_retroactively_cancel_admitted_read() {
    let database = TestDatabase::new();
    let grant_scope = scope();
    let mut provisioner = SqliteSubjectGrantStore::open(database.path()).expect("open store");
    provisioner
        .provision(&grant_spec(
            "grant-reserved",
            "https://issuer.example",
            "alice",
            FacadePermission::MetadataRead,
            &grant_scope,
            10,
            900,
        ))
        .expect("provision grant");
    drop(provisioner);

    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (continue_tx, continue_rx) = std::sync::mpsc::channel();
    let claims = claims("https://issuer.example", "alice", 900);
    let config = facade(grant_scope.clone());
    let request = http_request(&grant_scope);
    let mut ingress = make_ingress(database.path(), claims.clone(), None);
    let worker = std::thread::spawn(move || {
        let mut resolver = support::CredentialResolver {
            calls: Arc::new(AtomicUsize::new(0)),
            blocked: Some((entered_tx, continue_rx)),
        };
        ingress.admit_http(
            &request,
            &config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        )
    });

    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("resolver reached after grant reservation");
    let mut revoker = SqliteSubjectGrantStore::open(database.path()).expect("second handle");
    assert_eq!(revoker.revoke("grant-reserved"), Ok(1));
    continue_tx.send(()).expect("release resolver");
    let admitted = worker
        .join()
        .expect("admission worker")
        .expect("reservation completed before revocation");

    let mut service = HarnessBackedContextService::new_trusted(
        context_service::ControlPlane::new(grant_scope.clone(), true, 100)
            .expect("matching synthetic owner"),
        facade(grant_scope.clone()),
    )
    .expect("read-only trusted service");
    service
        .state(&admitted)
        .expect("a completed reservation is not retroactively cancelled");
}

fn make_ingress(
    path: &Path,
    principal: VerifiedPrincipalClaims,
    gate: Option<Arc<Barrier>>,
) -> AuthenticatedIngress<support::FixedVerifier, GatedGrantStore> {
    let store = SqliteSubjectGrantStore::open(path).expect("open ingress grant store");
    AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new(principal.issuer.clone(), principal.audience.clone())
            .expect("ingress config"),
        support::FixedVerifier(principal),
        GatedGrantStore { store, gate },
    )
}

struct GatedGrantStore {
    store: SqliteSubjectGrantStore,
    gate: Option<Arc<Barrier>>,
}

impl SubjectGrantStore for GatedGrantStore {
    fn reserve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &context_service::ControlScope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
    ) -> Result<Vec<context_service::AdmittedSubjectGrant>, SubjectGrantError> {
        if let Some(gate) = &self.gate {
            gate.wait();
        }
        self.store
            .reserve(principal, scope, required, optional, now)
    }
}
