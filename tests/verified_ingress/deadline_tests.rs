// SPDX-License-Identifier: MIT

use super::support::{FixedResolver, TestDatabase, claims, facade, ingress, request, scope};
use context_service::{
    AuthenticatedIngressError, FacadePermission, SqliteSubjectGrantStore, SubjectGrantSpec,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

#[test]
fn slow_owner_resolution_does_not_extend_admitted_authority_lifetime() {
    let database = TestDatabase::new();
    let scope = scope();
    let mut store = SqliteSubjectGrantStore::open(&database.path).expect("open grant store");
    store
        .provision(&SubjectGrantSpec {
            grant_id: "one-second-reader".to_owned(),
            issuer: super::support::ISSUER.to_owned(),
            subject: super::support::SUBJECT.to_owned(),
            permission: FacadePermission::MetadataRead,
            scope: scope.clone(),
            not_before: 100,
            expires_at: 101,
        })
        .expect("provision bounded grant");
    drop(store);

    let (entered_tx, entered_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let verifier_calls = Arc::new(AtomicUsize::new(0));
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let mut ingress = ingress(&database, claims(101), verifier_calls);
    let mut resolver = FixedResolver {
        calls: resolver_calls.clone(),
        blocked: Some((entered_tx, continue_rx)),
    };
    let request = request(&scope);
    let config = facade(scope);
    let worker = std::thread::spawn(move || {
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
        .expect("resolver reached after admission started");
    std::thread::sleep(Duration::from_millis(1100));
    continue_tx.send(()).expect("release resolver");
    assert_eq!(
        worker.join().expect("ingress worker"),
        Err(AuthenticatedIngressError::Unauthorized),
        "resolver time must count against the original monotonic deadline"
    );
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 1);
}
