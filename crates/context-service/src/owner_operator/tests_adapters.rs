#![cfg(unix)]

use super::config::{CONFIG_SCHEMA, OperatorConfig};
use super::principal::{FilePrincipalVerifier, test_tag};
use super::slots::{FileHarnessBearerRedeemer, FileOwnerSlotResolver};
use super::tests_files::{Fixture, write_private_at};
use crate::authenticated_ingress::{
    AuthenticatedIngress, AuthenticatedIngressConfig, PrincipalVerificationError,
    PrincipalVerifier, VerifiedPrincipal,
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
use crate::harness_owner_transport::{CredentialRedemptionError, HarnessCredentialRedeemer};
use crate::http::HttpRequest;
use crate::owner_invocation_store::AdmissionUse;
use crate::protected_owner_credentials::{
    OwnerCredentialDescriptor, ProtectedOwnerCredentialResolver, ResolvedOwnerCredential,
};
use crate::subject_grants::{AdmittedSubjectGrant, SubjectGrantError, SubjectGrantStore};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const ISSUER: &str = "https://identity.example/tenant/one";
const SUBJECT: &str = "user-1";
const AUDIENCE: &str = "https://console.example/api";
const PRINCIPAL_ID: &str = "credential-1";
const OWNER_ID: &str = "owner-test";
const OWNER_REF: &str = "harness-slot-1";
const TOKEN_SECRET: [u8; 32] = [0x71; 32];
const HARNESS_BEARER: &[u8] = b"harness-token-fixture";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("wall clock after epoch")
        .as_secs()
}

fn expires() -> u64 {
    4_102_444_800
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String");
    }
    output
}

fn bearer_token() -> String {
    format!("ccp1.principal.prod-1.{}", hex(&TOKEN_SECRET))
}

fn config_json(state_root: &Path) -> Value {
    json!({
        "schema_version": CONFIG_SCHEMA,
        "protected_source": "protected-private-files-v1",
        "listen_address": "127.0.0.1:18181",
        "harness_address": "[::1]:18282",
        "owner_id": OWNER_ID,
        "scope": {
            "project_id": "project-test",
            "run_id": "run-test",
            "episode_id": "episode-test",
            "agent_id": "agent-test"
        },
        "issuer": ISSUER,
        "audience": AUDIENCE,
        "expected_host": "console.test:18181",
        "expected_origin": "http://console.test:18181",
        "csrf_secret_ref": "csrf-secret.bin",
        "state_root": state_root,
        "principal_registry_ref": "principals.json",
        "principal_mac_key_ref": "principal-mac.bin",
        "owner_slots_ref": "owner-slots.json",
        "journal_key_manifest_ref": "journal-keys.json",
        "grant_database": "grants.sqlite3",
        "journal_database": "journal.sqlite3",
        "request_deadline_ms": 4500
    })
}

fn load_config(
    fixture: &Fixture,
    _state: &Path,
    value: Value,
) -> Result<OperatorConfig, super::protected_files::AdapterError> {
    let path = fixture.path().join("operator.json");
    let contents = serde_json::to_vec(&value).unwrap();
    if path.exists() {
        replace_private(&path, "operator.next", &contents);
    } else {
        write_private_at(fixture.path(), "operator.json", &contents);
    }
    OperatorConfig::load(path)
}

fn invocation_scope(invocation: &ContextOwnerInvocationV2) -> Scope {
    let scope = &invocation.identity.console_scope;
    Scope::new(
        scope.project_id.clone(),
        scope.run_id.clone(),
        scope.episode_id.clone(),
        scope.agent_id.clone(),
    )
    .unwrap()
}

fn scope() -> Scope {
    Scope::new("project-test", "run-test", "episode-test", "agent-test").unwrap()
}

fn invocation() -> ContextOwnerInvocationV2 {
    let expiration = expires();
    ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: OwnerIdentityCorrelationV2 {
            console: ConsoleOwnerIdentityV2 {
                issuer: ISSUER.to_owned(),
                subject: SUBJECT.to_owned(),
                audience: AUDIENCE.to_owned(),
                credential_id: PRINCIPAL_ID.to_owned(),
                grant_id: "grant-1".to_owned(),
                grant_generation: 1,
                grant_expires_at: expiration,
            },
            console_scope: ConsoleOwnerScopeV2 {
                project_id: "project-test".to_owned(),
                run_id: "run-test".to_owned(),
                episode_id: "episode-test".to_owned(),
                agent_id: "agent-test".to_owned(),
            },
            harness: HarnessActorIdentityV2 {
                actor_subject: SUBJECT.to_owned(),
                owner_id: OWNER_ID.to_owned(),
                workflow_run_id: "run-test".to_owned(),
                credential_reference_id: OWNER_REF.to_owned(),
                credential_expires_at: expiration,
            },
        },
        expected_binding: None,
        operation: ContextOwnerOperationV2::CurrentAssociation {
            workflow_run_id: "run-test".to_owned(),
        },
    }
}

fn principal_registry(expiration: u64, revoked: bool, key: &[u8; 32]) -> Vec<u8> {
    let tag = test_tag(
        key,
        "principal.prod-1",
        ISSUER,
        SUBJECT,
        AUDIENCE,
        PRINCIPAL_ID,
        expiration,
        revoked,
        &TOKEN_SECRET,
    );
    serde_json::to_vec(&json!({
        "schema_version": "ascension.context-console.principals.v1",
        "records": [{
            "reference": "principal.prod-1",
            "issuer": ISSUER,
            "subject": SUBJECT,
            "audience": AUDIENCE,
            "credential_id": PRINCIPAL_ID,
            "expires_at": expiration,
            "revoked": revoked,
            "tag": tag
        }]
    }))
    .unwrap()
}

fn principal_registry_with_duplicate_field(expiration: u64, key: &[u8; 32]) -> Vec<u8> {
    let tag = test_tag(
        key,
        "principal.prod-1",
        ISSUER,
        SUBJECT,
        AUDIENCE,
        PRINCIPAL_ID,
        expiration,
        false,
        &TOKEN_SECRET,
    );
    format!(
        r#"{{"schema_version":"ascension.context-console.principals.v1","records":[{{"reference":"principal.prod-1","reference":"principal.prod-1","issuer":"{ISSUER}","subject":"{SUBJECT}","audience":"{AUDIENCE}","credential_id":"{PRINCIPAL_ID}","expires_at":{expiration},"revoked":false,"tag":"{tag}"}}]}}"#
    )
    .into_bytes()
}

fn slot_registry(reference: &str, bearer_file: &str, bearer: &[u8], revoked: bool) -> Vec<u8> {
    let digest = hex(&Sha256::digest(bearer));
    serde_json::to_vec(&json!({
        "schema_version": "ascension.context-console.owner-slots.v1",
        "slots": [{
            "owner_id": OWNER_ID,
            "reference": reference,
            "console_issuer": ISSUER,
            "console_subject": SUBJECT,
            "console_audience": AUDIENCE,
            "console_credential_id": PRINCIPAL_ID,
            "revoked": revoked,
            "harness_subject": SUBJECT,
            "project_id": "project-test",
            "run_id": "run-test",
            "episode_id": "episode-test",
            "agent_id": "agent-test",
            "expires_at": expires(),
            "exact_harness_scopes": ["workflow:read"],
            "bearer_file_ref": bearer_file,
            "bearer_sha256": digest
        }]
    }))
    .unwrap()
}

fn setup_config(fixture: &Fixture) -> (OperatorConfig, std::path::PathBuf) {
    let state = fixture.private_dir("state");
    let value = config_json(&state);
    (
        load_config(fixture, &state, value).expect("strict operator config"),
        state,
    )
}

fn install_principal_files(state: &Path, expiration: u64, revoked: bool) {
    let key = [0x41; 32];
    write_private_at(state, "principal-mac.bin", &key);
    write_private_at(
        state,
        "principals.json",
        &principal_registry(expiration, revoked, &key),
    );
}

fn install_slot_files(state: &Path, reference: &str, revoked: bool) {
    write_private_at(state, "harness-bearer.bin", HARNESS_BEARER);
    write_private_at(
        state,
        "owner-slots.json",
        &slot_registry(reference, "harness-bearer.bin", HARNESS_BEARER, revoked),
    );
}

fn replace_private(path: &Path, temp_name: &str, contents: &[u8]) {
    let parent = path.parent().unwrap();
    write_private_at(parent, temp_name, contents);
    fs::rename(parent.join(temp_name), path).expect("atomically replace private registry");
}

#[test]
fn operator_config_is_closed_numeric_and_rejects_duplicate_or_ambiguous_references() {
    let fixture = Fixture::new();
    let state = fixture.private_dir("state");
    let base = config_json(&state);
    assert!(load_config(&fixture, &state, base.clone()).is_ok());

    let mut unknown = base.clone();
    unknown["secret"] = json!("must-not-be-configured");
    assert!(load_config(&fixture, &state, unknown).is_err());

    let mut nonnumeric = base.clone();
    nonnumeric["listen_address"] = json!("localhost:18181");
    assert!(load_config(&fixture, &state, nonnumeric).is_err());

    let mut nonloopback = base.clone();
    nonloopback["harness_address"] = json!("192.0.2.1:18282");
    assert!(load_config(&fixture, &state, nonloopback).is_err());

    let mut ambiguous = base.clone();
    ambiguous["owner_slots_ref"] = json!("principals.json");
    assert!(load_config(&fixture, &state, ambiguous).is_err());

    let mut aliases_wal = base.clone();
    aliases_wal["csrf_secret_ref"] = json!("grants.sqlite3-wal");
    assert!(load_config(&fixture, &state, aliases_wal).is_err());

    let mut aliases_journal = base.clone();
    aliases_journal["principal_mac_key_ref"] = json!("journal.sqlite3-journal");
    assert!(load_config(&fixture, &state, aliases_journal).is_err());

    let mut overlapping_database_artifacts = base.clone();
    overlapping_database_artifacts["journal_database"] = json!("grants.sqlite3-wal");
    assert!(load_config(&fixture, &state, overlapping_database_artifacts).is_err());

    let duplicate =
        format!(r#"{{"schema_version":"{CONFIG_SCHEMA}","schema_version":"{CONFIG_SCHEMA}"}}"#);
    replace_private(
        &fixture.path().join("operator.json"),
        "duplicate.next",
        duplicate.as_bytes(),
    );
    assert!(OperatorConfig::load(fixture.path().join("operator.json")).is_err());
}

#[test]
fn principal_verification_uses_current_signed_registry_and_rejects_bad_or_revoked_entries() {
    let fixture = Fixture::new();
    let (config, state) = setup_config(&fixture);
    let expiration = expires();
    install_principal_files(&state, expiration, false);
    let bearer = bearer_token();
    assert!(
        !fs::read(state.join("principals.json"))
            .unwrap()
            .windows(bearer.len())
            .any(|window| window == bearer.as_bytes())
    );

    let mut verifier = FilePrincipalVerifier::new(&config);
    let claims = verifier
        .verify(bearer.as_bytes())
        .expect("current HMAC record");
    assert_eq!(claims.issuer, ISSUER);
    assert_eq!(claims.subject, SUBJECT);
    assert_eq!(claims.credential_id, PRINCIPAL_ID);
    assert!(
        verifier
            .verify(b"ccp1.principal-1.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
            .is_err()
    );

    let key = [0x41; 32];
    replace_private(
        &state.join("principals.json"),
        "revoked.next",
        &principal_registry(expiration, true, &key),
    );
    assert_eq!(
        verifier.verify(bearer.as_bytes()),
        Err(PrincipalVerificationError::Invalid)
    );

    replace_private(
        &state.join("principals.json"),
        "tampered.next",
        &principal_registry(expiration, false, &[0x42; 32]),
    );
    assert_eq!(
        verifier.verify(bearer.as_bytes()),
        Err(PrincipalVerificationError::Invalid)
    );

    replace_private(
        &state.join("principals.json"),
        "expired.next",
        &principal_registry(now().saturating_sub(1), false, &key),
    );
    assert_eq!(
        verifier.verify(bearer.as_bytes()),
        Err(PrincipalVerificationError::Invalid)
    );

    replace_private(
        &state.join("principals.json"),
        "duplicate.next",
        &principal_registry_with_duplicate_field(expiration, &key),
    );
    assert_eq!(
        verifier.verify(bearer.as_bytes()),
        Err(PrincipalVerificationError::Invalid)
    );
}

struct FixedGrantStore;

impl SubjectGrantStore for FixedGrantStore {
    fn reserve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        _now: u64,
    ) -> Result<Vec<AdmittedSubjectGrant>, SubjectGrantError> {
        if required != [FacadePermission::MetadataRead] || !optional.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![AdmittedSubjectGrant {
            grant_id: "grant-1".to_owned(),
            issuer: principal.issuer().to_owned(),
            subject: principal.subject().to_owned(),
            permission: FacadePermission::MetadataRead,
            scope: scope.clone(),
            not_before: 0,
            expires_at: expires(),
            revocation_generation: 1,
        }])
    }
}

fn admit_with_file_resolver(
    config: &OperatorConfig,
    invocation: &ContextOwnerInvocationV2,
    token: &str,
) -> Result<
    crate::authenticated_ingress::AuthenticatedOwnerInvocation,
    crate::authenticated_ingress::AuthenticatedIngressError,
> {
    let mut ingress = AuthenticatedIngress::new(
        AuthenticatedIngressConfig::new(ISSUER, AUDIENCE).unwrap(),
        FilePrincipalVerifier::new(config),
        FixedGrantStore,
    );
    let mut resolver = FileOwnerSlotResolver::new(config);
    let request = HttpRequest {
        method: "POST".to_owned(),
        target: "/v1/context-owner/invocations".to_owned(),
        headers: vec![
            ("authorization".to_owned(), format!("Bearer {token}")),
            ("host".to_owned(), "console.test:18181".to_owned()),
            ("origin".to_owned(), "http://console.test:18181".to_owned()),
        ],
        body: serde_json::to_vec(invocation).unwrap(),
    };
    let facade = HarnessFacadeConfig::new(
        invocation_scope(invocation),
        "console.test:18181",
        Some("http://console.test:18181".to_owned()),
        None,
        RetentionPolicy::default(),
    )
    .unwrap();
    ingress.admit_owner_invocation(
        &request,
        &facade,
        invocation,
        AdmissionUse::ReadOnly,
        &mut resolver,
    )
}

#[test]
fn resolver_binds_principal_full_scope_and_rejects_ambiguous_or_revoked_slots() {
    let fixture = Fixture::new();
    let (config, state) = setup_config(&fixture);
    let expiration = expires();
    install_principal_files(&state, expiration, false);
    install_slot_files(&state, OWNER_REF, false);
    let token = bearer_token();
    let admitted = admit_with_file_resolver(&config, &invocation(), &token)
        .expect("exact principal, scope and slot resolve");
    assert_eq!(admitted.credential().reference().as_str(), OWNER_REF);

    let mut wrong_scope = invocation();
    wrong_scope.identity.console_scope.project_id = "another-project".to_owned();
    assert!(admit_with_file_resolver(&config, &wrong_scope, &token).is_err());

    let mut two = json!({
        "schema_version": "ascension.context-console.owner-slots.v1",
        "slots": [
            serde_json::from_slice::<Value>(&slot_registry(OWNER_REF, "harness-bearer.bin", HARNESS_BEARER, false)).unwrap()["slots"][0].clone(),
            serde_json::from_slice::<Value>(&slot_registry("harness-slot-2", "second-bearer.bin", HARNESS_BEARER, false)).unwrap()["slots"][0].clone()
        ]
    });
    let second = two["slots"][1].as_object_mut().unwrap();
    second.insert("bearer_file_ref".to_owned(), json!("second-bearer.bin"));
    write_private_at(&state, "second-bearer.bin", HARNESS_BEARER);
    replace_private(
        &state.join("owner-slots.json"),
        "ambiguous.next",
        &serde_json::to_vec(&two).unwrap(),
    );
    assert!(admit_with_file_resolver(&config, &invocation(), &token).is_err());

    replace_private(
        &state.join("owner-slots.json"),
        "revoked.next",
        &slot_registry(OWNER_REF, "harness-bearer.bin", HARNESS_BEARER, true),
    );
    assert!(admit_with_file_resolver(&config, &invocation(), &token).is_err());
}

fn make_descriptor(expiration: u64, scopes: &[&str]) -> OwnerCredentialDescriptor {
    OwnerCredentialDescriptor::new(
        ISSUER,
        SUBJECT,
        SUBJECT,
        scope(),
        scopes
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>(),
        expiration,
    )
}

#[test]
fn redeemer_reloads_exact_descriptor_scope_reference_and_bearer_digest() {
    let fixture = Fixture::new();
    let (config, state) = setup_config(&fixture);
    install_slot_files(&state, OWNER_REF, false);
    install_valid_journal_keys(&state);
    let invocation = invocation();
    let reference = ProtectedAuthReference::new(OWNER_REF).unwrap();
    let descriptor = make_descriptor(
        invocation.identity.harness.credential_expires_at,
        &["workflow:read"],
    );
    let mut redeemer = FileHarnessBearerRedeemer::new(&config);
    let token = redeemer
        .redeem(
            &reference,
            &descriptor,
            &invocation,
            &["workflow:read"],
            now(),
        )
        .expect("current exact slot bearer");
    assert_eq!(token.as_slice(), HARNESS_BEARER);

    let mut changed = invocation.clone();
    changed.identity.harness.credential_reference_id = "another-slot".to_owned();
    assert!(matches!(
        redeemer.redeem(&reference, &descriptor, &changed, &["workflow:read"], now()),
        Err(CredentialRedemptionError::Denied)
    ));
    assert!(matches!(
        redeemer.redeem(&reference, &descriptor, &invocation, &["workflow:*"], now()),
        Err(CredentialRedemptionError::Denied)
    ));
    assert!(matches!(
        redeemer.redeem(
            &reference,
            &make_descriptor(
                invocation.identity.harness.credential_expires_at + 1,
                &["workflow:read"]
            ),
            &invocation,
            &["workflow:read"],
            now()
        ),
        Err(CredentialRedemptionError::Denied)
    ));

    let duplicate = String::from_utf8(slot_registry(
        OWNER_REF,
        "harness-bearer.bin",
        HARNESS_BEARER,
        false,
    ))
    .unwrap()
    .replace(
        "\"reference\":\"harness-slot-1\"",
        "\"reference\":\"harness-slot-1\",\"reference\":\"harness-slot-1\"",
    );
    replace_private(
        &state.join("owner-slots.json"),
        "duplicate-slot.next",
        duplicate.as_bytes(),
    );
    assert!(matches!(
        redeemer.redeem(
            &reference,
            &descriptor,
            &invocation,
            &["workflow:read"],
            now()
        ),
        Err(CredentialRedemptionError::Denied)
    ));

    replace_private(
        &state.join("owner-slots.json"),
        "valid-restored.next",
        &slot_registry(OWNER_REF, "harness-bearer.bin", HARNESS_BEARER, false),
    );
    replace_private(
        &state.join("harness-bearer.bin"),
        "bearer.next",
        b"other-secret",
    );
    assert!(matches!(
        redeemer.redeem(
            &reference,
            &descriptor,
            &invocation,
            &["workflow:read"],
            now()
        ),
        Err(CredentialRedemptionError::Denied)
    ));

    replace_private(
        &state.join("owner-slots.json"),
        "revoked.next",
        &slot_registry(OWNER_REF, "harness-bearer.bin", HARNESS_BEARER, true),
    );
    assert!(matches!(
        redeemer.redeem(
            &reference,
            &descriptor,
            &invocation,
            &["workflow:read"],
            now()
        ),
        Err(CredentialRedemptionError::Denied)
    ));
}

fn install_key_material(state: &Path) {
    write_private_at(state, "index.bin", &[0x12; 32]);
    write_private_at(state, "data-old.bin", &[0x23; 32]);
    write_private_at(state, "data-current.bin", &[0x34; 32]);
}

fn install_valid_journal_keys(state: &Path) {
    install_key_material(state);
    write_private_at(
        state,
        "journal-keys.json",
        &key_manifest(json!([
            {"key_id":"data-v1","file_ref":"data-old.bin"},
            {"key_id":"data-v2","file_ref":"data-current.bin"}
        ])),
    );
}

fn key_manifest(entries: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema_version": "ascension.context-console.owner-journal-keys.v1",
        "index_key_id": "index-v1",
        "index_key_ref": "index.bin",
        "current_data_key_id": "data-v2",
        "data_keys": entries
    }))
    .unwrap()
}

#[path = "tests_adapters/keys.rs"]
mod keys;
