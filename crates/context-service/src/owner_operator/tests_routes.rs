use super::routes::{RouteSelectionError, RouteUse, select_route};

#[test]
fn production_route_selector_accepts_only_exact_v2_post_paths() {
    let accepted = [
        ("/v2/context-owner/invocations", RouteUse::Invoke),
        (
            "/v2/context-owner/invocations/receipt-lookup",
            RouteUse::ExactLookup,
        ),
        (
            "/v2/context-owner/invocations/cached-result",
            RouteUse::CachedRead,
        ),
    ];
    for (target, expected) in accepted {
        assert_eq!(select_route("POST", target), Ok(expected), "{target}");
    }

    for target in [
        "/v2/context-owner/invocations",
        "/v2/context-owner/invocations/not-a-route",
    ] {
        assert_eq!(
            select_route("GET", target).map_err(RouteSelectionError::http_error),
            Err((405, &br#"{"error":"method_not_allowed"}"#[..])),
            "{target}"
        );
    }

    for target in [
        "/v1/context-owner/invocations",
        "/v2/context-owner/invocations/recover",
        "/v2/context-owner/invocations/recovery",
        "/v2/context-owner/invocations/receipt-lookup?request_id=known",
        "/V2/context-owner/invocations",
        "/v2/context-owner/invocations/not-a-route",
    ] {
        assert_eq!(
            select_route("POST", target).map_err(RouteSelectionError::http_error),
            Err((404, &br#"{"error":"route_unavailable"}"#[..])),
            "{target}"
        );
    }
}

#[cfg(unix)]
#[test]
fn sqlite_generation_zero_admits_exact_row_and_wrong_generation_is_denied_at_ingress() {
    use super::tests_files::Fixture;
    use crate::authenticated_ingress::{
        AuthenticatedIngress, AuthenticatedIngressConfig, AuthenticatedIngressError,
        PrincipalVerificationError, PrincipalVerifier, VerifiedPrincipalClaims,
    };
    use crate::control::Scope;
    use crate::harness_context_owner_wire::{
        CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2, ConsoleOwnerIdentityV2, ConsoleOwnerScopeV2,
        ContextOwnerInvocationV2, ContextOwnerOperationV2, HarnessActorIdentityV2,
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
    use crate::subject_grants::{SqliteSubjectGrantStore, SubjectGrantSpec};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Verifier(VerifiedPrincipalClaims);
    impl PrincipalVerifier for Verifier {
        fn verify(
            &mut self,
            _bearer: &[u8],
        ) -> Result<VerifiedPrincipalClaims, PrincipalVerificationError> {
            Ok(self.0.clone())
        }
    }

    struct Resolver {
        expiry: u64,
    }
    impl ProtectedOwnerCredentialResolver for Resolver {
        fn resolve(
            &mut self,
            principal: &crate::authenticated_ingress::VerifiedPrincipal,
            scope: &Scope,
            scopes: &[&'static str],
            now: u64,
        ) -> Result<ResolvedOwnerCredential, CredentialResolutionError> {
            if scopes != ["workflow:read"] || self.expiry <= now {
                return Err(CredentialResolutionError::Denied);
            }
            let descriptor = OwnerCredentialDescriptor::new(
                principal.issuer().to_owned(),
                principal.subject().to_owned(),
                principal.subject().to_owned(),
                scope.clone(),
                vec!["workflow:read".to_owned()],
                self.expiry,
            );
            Ok(ResolvedOwnerCredential::new(
                ProtectedAuthReference::new("slot-read-1").expect("valid reference"),
                descriptor,
            ))
        }
    }

    let fixture = Fixture::new();
    let scope = Scope::new("project-a", "run-a", "episode-a", "agent-a").expect("closed scope");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_secs()
        + 1;
    let grant_expiry = now + 900;
    let principal_expiry = now + 1_800;
    let owner_expiry = now + 800;
    let db = fixture.path().join("grants.sqlite3");
    let mut store = SqliteSubjectGrantStore::open(&db).expect("real SQLite grant store");
    store
        .provision(&SubjectGrantSpec {
            grant_id: "grant-a".to_owned(),
            issuer: "issuer-a".to_owned(),
            subject: "actor-a".to_owned(),
            permission: FacadePermission::MetadataRead,
            scope: scope.clone(),
            not_before: now - 1,
            expires_at: grant_expiry,
        })
        .expect("public provisioning seam");

    let mut invocation = ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: OwnerIdentityCorrelationV2 {
            console: ConsoleOwnerIdentityV2 {
                issuer: "issuer-a".to_owned(),
                subject: "actor-a".to_owned(),
                audience: "console-a".to_owned(),
                credential_id: "credential-a".to_owned(),
                grant_id: "grant-a".to_owned(),
                grant_generation: 0,
                grant_expires_at: grant_expiry,
            },
            console_scope: ConsoleOwnerScopeV2 {
                project_id: scope.project_id.clone(),
                run_id: scope.run_id.clone(),
                episode_id: scope.episode_id.clone(),
                agent_id: scope.agent_id.clone(),
            },
            harness: HarnessActorIdentityV2 {
                actor_subject: "actor-a".to_owned(),
                owner_id: "owner-a".to_owned(),
                workflow_run_id: "run-a".to_owned(),
                credential_reference_id: "slot-read-1".to_owned(),
                credential_expires_at: owner_expiry,
            },
        },
        expected_binding: None,
        operation: ContextOwnerOperationV2::CurrentAssociation {
            workflow_run_id: "run-a".to_owned(),
        },
    };
    let facade = HarnessFacadeConfig::new(
        scope,
        "console.example",
        None,
        None,
        RetentionPolicy::default(),
    )
    .expect("read-only facade");
    let verifier = Verifier(VerifiedPrincipalClaims {
        issuer: "issuer-a".to_owned(),
        subject: "actor-a".to_owned(),
        audience: "console-a".to_owned(),
        credential_id: "credential-a".to_owned(),
        expires_at: principal_expiry,
    });
    let mut ingress = AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new("issuer-a", "console-a").expect("ingress config"),
        verifier,
        store,
    );
    let mut resolver = Resolver {
        expiry: owner_expiry,
    };
    let admitted = ingress
        .admit_owner_invocation(
            &request(&invocation),
            &facade,
            &invocation,
            AdmissionUse::ReadOnly,
            &mut resolver,
        )
        .expect("initial real SQLite row uses exact generation zero");
    assert_eq!(admitted.invocation().identity.console.grant_generation, 0);

    invocation.identity.console.grant_generation = 1;
    let denied = ingress.admit_owner_invocation(
        &request(&invocation),
        &facade,
        &invocation,
        AdmissionUse::ReadOnly,
        &mut resolver,
    );
    assert_eq!(denied.err(), Some(AuthenticatedIngressError::GrantDenied));

    fn request(invocation: &ContextOwnerInvocationV2) -> HttpRequest {
        HttpRequest {
            method: "POST".to_owned(),
            target: "/v2/context-owner/invocations".to_owned(),
            headers: vec![
                (
                    "authorization".to_owned(),
                    "Bearer source-fixture".to_owned(),
                ),
                ("host".to_owned(), "console.example".to_owned()),
            ],
            body: serde_json::to_vec(invocation).expect("serialize typed request"),
        }
    }
}

#[cfg(unix)]
#[test]
fn operator_config_reserves_lock_name_from_all_secret_and_database_roles() {
    use super::config::{CONFIG_SCHEMA, OperatorConfig};
    use super::protected_files::AdapterError;
    use super::tests_files::{Fixture, write_private_at};
    use serde_json::json;

    let fixture = Fixture::new();
    let state = fixture.private_dir("state");
    let config_path = fixture.path().join("owner-config.json");
    let base = json!({
        "schema_version": CONFIG_SCHEMA,
        "protected_source": "protected-private-files-v1",
        "listen_address": "127.0.0.1:18181",
        "harness_address": "[::1]:18282",
        "owner_id": "owner-test",
        "scope": {"project_id":"project-a","run_id":"run-a","episode_id":"episode-a","agent_id":"agent-a"},
        "issuer": "issuer-a",
        "audience": "audience-a",
        "expected_host": "console.example",
        "expected_origin": null,
        "csrf_secret_ref": "csrf.bin",
        "state_root": state,
        "principal_registry_ref": "principal.json",
        "principal_mac_key_ref": "principal-mac.bin",
        "owner_slots_ref": "slots.json",
        "journal_key_manifest_ref": "journal-keys.json",
        "grant_database": "grants.sqlite3",
        "journal_database": "journal.sqlite3",
        "request_deadline_ms": 5000
    });
    let mut lock_secret = base.clone();
    lock_secret["csrf_secret_ref"] = json!("owner-service.lock");
    write_private_at(
        fixture.path(),
        "owner-config.json",
        &serde_json::to_vec(&lock_secret).expect("config JSON"),
    );
    assert_eq!(
        OperatorConfig::load(&config_path).err(),
        Some(AdapterError::Invalid)
    );

    std::fs::remove_file(&config_path).expect("remove test config");
    let mut lock_database = base;
    lock_database["grant_database"] = json!("owner-service.lock");
    write_private_at(
        fixture.path(),
        "owner-config.json",
        &serde_json::to_vec(&lock_database).expect("config JSON"),
    );
    assert_eq!(
        OperatorConfig::load(&config_path).err(),
        Some(AdapterError::Invalid)
    );
}
