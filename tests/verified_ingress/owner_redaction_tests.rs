// SPDX-License-Identifier: MIT

use super::support::{
    FixedResolver, ISSUER, SUBJECT, TestDatabase, UnavailableResolver, claims, facade, ingress,
    request, scope,
};
use context_service::{
    AuthenticatedIngressError, ControlPlane, FacadePermission, HarnessBackedContextService,
    OwnerCredentialDescriptor, ProtectedAuthReference, ResolvedOwnerCredential,
    SqliteSubjectGrantStore, SubjectGrantSpec,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn verified_request_uses_subject_grant_and_redacts_bearer_and_owner_reference() {
    let database = TestDatabase::new();
    let scope_value = scope();
    let mut store = SqliteSubjectGrantStore::open(&database.path).expect("open grant store");
    store
        .provision(&SubjectGrantSpec {
            grant_id: "metadata-reader".to_owned(),
            issuer: ISSUER.to_owned(),
            subject: SUBJECT.to_owned(),
            permission: FacadePermission::MetadataRead,
            scope: scope_value.clone(),
            not_before: 50,
            expires_at: 800,
        })
        .expect("provision subject grant");
    drop(store);

    let unavailable_calls = Arc::new(AtomicUsize::new(0));
    let mut unavailable_ingress = ingress(&database, claims(900), Arc::new(AtomicUsize::new(0)));
    let mut unavailable_resolver = UnavailableResolver {
        calls: unavailable_calls.clone(),
    };
    assert_eq!(
        unavailable_ingress.admit_http(
            &request(&scope_value),
            &facade(scope_value.clone()),
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut unavailable_resolver,
        ),
        Err(AuthenticatedIngressError::OwnerCredentialUnavailable)
    );
    assert_eq!(unavailable_calls.load(Ordering::SeqCst), 1);
    drop(unavailable_ingress);

    let verifier_calls = Arc::new(AtomicUsize::new(0));
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let mut ingress = ingress(&database, claims(900), verifier_calls.clone());
    let mut resolver = FixedResolver {
        calls: resolver_calls.clone(),
        blocked: None,
    };
    let request = request(&scope_value);
    let admitted = ingress
        .admit_http(
            &request,
            &facade(scope_value.clone()),
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        )
        .expect("verified grant and credential admission");
    assert_eq!(admitted.principal(), SUBJECT);
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 1);

    let debug_request = format!("{admitted:?}");
    assert!(!debug_request.contains(super::support::BEARER));
    assert!(!debug_request.contains("synthetic-owner-reference"));

    let mut service = HarnessBackedContextService::new_trusted(
        ControlPlane::new(scope_value.clone(), true, 100).expect("matching synthetic owner"),
        facade(scope_value),
    )
    .expect("trusted composition performs no constructor owner I/O");
    let state = service
        .state(&admitted)
        .expect("admitted read reaches owner");
    assert_eq!(state.scope, scope());

    let credential = ResolvedOwnerCredential::new(
        ProtectedAuthReference::new("synthetic-owner-reference").expect("opaque ref"),
        OwnerCredentialDescriptor::new(
            ISSUER,
            SUBJECT,
            SUBJECT,
            state.scope,
            ["workflow:read".to_owned()],
            800,
        ),
    );
    assert!(!format!("{credential:?}").contains("synthetic-owner-reference"));
}
