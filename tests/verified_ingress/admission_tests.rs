// SPDX-License-Identifier: MIT

use super::support::{
    BEARER, FixedResolver, TestDatabase, claims, facade, ingress, request, scope,
};
use context_service::{AuthenticatedIngressError, FacadePermission};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn malformed_or_expired_request_is_refused_before_owner_credential_resolution() {
    let database = TestDatabase::new();
    let scope = scope();
    let verifier_calls = Arc::new(AtomicUsize::new(0));
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let mut ingress = ingress(&database, claims(99), verifier_calls.clone());
    let mut resolver = FixedResolver {
        calls: resolver_calls.clone(),
        blocked: None,
    };
    let config = facade(scope.clone());
    let mut request = request(&scope);

    request.headers.push((
        "x-principal".to_owned(),
        "attacker-selected-user".to_owned(),
    ));
    assert_eq!(
        ingress.admit_http(
            &request,
            &config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::InvalidHttpRequest)
    );
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 0);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);

    request.headers.pop();
    request
        .headers
        .push(("authorization".to_owned(), format!("Bearer {BEARER}")));
    assert_eq!(
        ingress.admit_http(
            &request,
            &config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::InvalidHttpRequest),
        "duplicate authentication fields are refused before verifier or owner lookup"
    );
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 0);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);

    request.headers.pop();
    request
        .headers
        .iter_mut()
        .find(|(name, _)| name == "authorization")
        .expect("authorization header")
        .1 = "Basic invalid-scheme-token".to_owned();
    assert_eq!(
        ingress.admit_http(
            &request,
            &config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::InvalidBearer)
    );
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 0);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);

    request
        .headers
        .iter_mut()
        .find(|(name, _)| name == "authorization")
        .expect("authorization header")
        .1 = format!("Bearer {BEARER}");
    assert_eq!(
        ingress.admit_http(
            &request,
            &config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::Unauthorized),
        "expired verified claims are refused before grant and owner resolution"
    );
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);

    let mut missing = request.clone();
    missing.headers.retain(|(name, _)| name != "authorization");
    assert_eq!(
        ingress.admit_http(
            &missing,
            &config,
            &[FacadePermission::MetadataRead],
            &[],
            100,
            &mut resolver,
        ),
        Err(AuthenticatedIngressError::InvalidBearer)
    );
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 1);
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 0);
}
