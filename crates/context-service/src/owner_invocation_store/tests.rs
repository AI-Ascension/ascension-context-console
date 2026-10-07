use super::*;
use crate::harness_context_owner_wire::*;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[path = "tests_admission.rs"]
mod admission_tests;

#[derive(Clone)]
struct TestKeyState {
    index_id: String,
    index_key: [u8; 32],
    current_id: String,
    data_keys: BTreeMap<String, [u8; 32]>,
}

#[derive(Clone)]
struct TestProvider(Arc<Mutex<TestKeyState>>);

impl TestProvider {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(TestKeyState {
            index_id: "test-index-v1".to_owned(),
            index_key: [0x11; 32],
            current_id: "data-v1".to_owned(),
            data_keys: BTreeMap::from([("data-v1".to_owned(), [0x22; 32])]),
        })))
    }

    fn rotate_data(&self) {
        let mut keys = self.0.lock().expect("test key lock");
        keys.data_keys.insert("data-v2".to_owned(), [0x33; 32]);
        keys.current_id = "data-v2".to_owned();
    }

    fn lose_old_data_key(&self) {
        let mut keys = self.0.lock().expect("test key lock");
        keys.data_keys.remove("data-v1");
        keys.current_id = "data-v2".to_owned();
        keys.data_keys.insert("data-v2".to_owned(), [0x33; 32]);
    }

    fn restore_old_data_key(&self) {
        let mut keys = self.0.lock().expect("test key lock");
        keys.data_keys.insert("data-v1".to_owned(), [0x22; 32]);
        keys.current_id = "data-v1".to_owned();
    }

    fn replace_existing_data_key_bytes(&self) {
        let mut keys = self.0.lock().expect("test key lock");
        keys.data_keys.insert("data-v1".to_owned(), [0x44; 32]);
        keys.current_id = "data-v1".to_owned();
    }

    fn replace_index_key_bytes_without_changing_id(&self) {
        let mut keys = self.0.lock().expect("test key lock");
        keys.index_key = [0x55; 32];
    }
}

impl OwnerInvocationKeyProvider for TestProvider {
    fn load(&mut self) -> Result<OwnerInvocationKeyMaterial, StoreError> {
        let keys = self.0.lock().map_err(|_| StoreError::KeyUnavailable)?;
        OwnerInvocationKeyMaterial::new(
            keys.index_id.clone(),
            keys.index_key,
            keys.current_id.clone(),
            keys.data_keys.clone(),
        )
    }
}

struct PrivateTempStore {
    _fixture: crate::owner_test_fixtures::PrivateTestDirectory,
    database: PathBuf,
}

impl PrivateTempStore {
    fn new() -> Self {
        let fixture = crate::owner_test_fixtures::PrivateTestDirectory::new("store");
        let database = fixture.path().join("intent.sqlite3");
        Self {
            _fixture: fixture,
            database,
        }
    }

    fn path(&self) -> &Path {
        &self.database
    }
}

fn boundary() -> ContextBoundary {
    ContextBoundary {
        run_id: "run-test".to_owned(),
        episode_id: "episode-test".to_owned(),
        agent_id: "agent-test".to_owned(),
        state_id: "state-test".to_owned(),
        generation: 42,
        observation_sha256: "1".repeat(64),
        catalog_sha256: "2".repeat(64),
        adapter_revision: "adapter.v1".to_owned(),
        model_revision: "model.v2".to_owned(),
        configuration_sha256: "3".repeat(64),
        output_schema_sha256: "4".repeat(64),
        controller_epoch: 3,
        gate_epoch: 5,
        control_version: 9,
    }
}

fn binding() -> ContextOwnerBinding {
    ContextOwnerBinding {
        schema_version: CONTEXT_OWNER_BINDING_SCHEMA_V1.to_owned(),
        owner_id: "owner-test".to_owned(),
        owner_version: "owner.v1".to_owned(),
        invocation_id: "invocation-test".to_owned(),
        binding_id: "binding-test".to_owned(),
        binding_version: 1,
        binding_digest: "a".repeat(64),
        context_ref: "context-test".to_owned(),
        instance_id: "instance-test".to_owned(),
        node_kind: "context-owner".to_owned(),
        state: ContextBindingState::Available,
        workflow_run_id: "run-test".to_owned(),
        definition_digest: "b".repeat(64),
        graph_id: "graph-test".to_owned(),
        node_id: "node-test".to_owned(),
        node_execution_id: "execution-test".to_owned(),
        boundary: boundary(),
        lease_epoch: 1,
        snapshot_id: "snapshot-test".to_owned(),
        approved_revision_id: "revision-4".to_owned(),
        plan_epoch: 7,
        grants: ContextBindingGrants {
            metadata_read: true,
            content_read: true,
            edit: true,
            control: true,
        },
        continuity: ContextBindingContinuity {
            survives_controller_restart: true,
            receipt_recovery: true,
            provider_session_continuity: false,
        },
    }
}

fn invocation(request_id: &str, note: &str) -> ContextOwnerInvocationV2 {
    let expected_boundary = boundary();
    ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: OwnerIdentityCorrelationV2 {
            console: ConsoleOwnerIdentityV2 {
                issuer: "issuer-test".to_owned(),
                subject: "user-test".to_owned(),
                audience: "console-test".to_owned(),
                credential_id: "console-credential-v1".to_owned(),
                grant_id: "grant-edit-v1".to_owned(),
                grant_generation: 1,
                grant_expires_at: 2_000_000_000,
            },
            console_scope: ConsoleOwnerScopeV2 {
                project_id: "project-test".to_owned(),
                run_id: "run-test".to_owned(),
                episode_id: "episode-test".to_owned(),
                agent_id: "agent-test".to_owned(),
            },
            harness: HarnessActorIdentityV2 {
                actor_subject: "user-test".to_owned(),
                owner_id: "owner-test".to_owned(),
                workflow_run_id: "run-test".to_owned(),
                credential_reference_id: "vault-reference-v1".to_owned(),
                credential_expires_at: 2_000_000_000,
            },
        },
        expected_binding: Some(binding()),
        operation: ContextOwnerOperationV2::PatchDraft {
            workflow_run_id: "run-test".to_owned(),
            draft_id: "draft-test".to_owned(),
            request: HarnessContextOwnerDraftPatchRequest {
                schema_version: CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1.to_owned(),
                request_id: request_id.to_owned(),
                draft_id: "draft-test".to_owned(),
                expected_version: 7,
                expected_boundary,
                operations: vec![HarnessContextOwnerDraftOperation::PutNote {
                    note_id: "note-test".to_owned(),
                    text: note.to_owned(),
                }],
            },
        },
    }
}

fn receipt_bytes(invocation: &ContextOwnerInvocationV2) -> Vec<u8> {
    let ContextOwnerOperationV2::PatchDraft { request, .. } = &invocation.operation else {
        panic!("patch fixture required");
    };
    let binding = invocation
        .expected_binding
        .clone()
        .expect("fixture binding");
    let draft = ContextDraft {
        schema: "ascension.context-control.draft.v1".to_owned(),
        draft_id: request.draft_id.clone(),
        version: request.expected_version + 1,
        base_revision_id: binding.approved_revision_id.clone(),
        selected_items: Vec::new(),
        pinned_item_ids: Vec::new(),
        notes: Vec::new(),
        objective: None,
        author_ref: invocation.identity.console.subject.clone(),
    };
    let receipt = HarnessContextOwnerMutationReceipt {
        schema_version: CONTEXT_OWNER_MUTATION_RECEIPT_SCHEMA_V1.to_owned(),
        owner_id: binding.owner_id.clone(),
        workflow_run_id: binding.workflow_run_id.clone(),
        actor_subject: invocation.identity.console.subject.clone(),
        binding_id: binding.binding_id.clone(),
        binding_digest: binding.binding_digest.clone(),
        invocation_id: binding.invocation_id.clone(),
        boundary: binding.boundary.clone(),
        operation: "patch_draft".to_owned(),
        request_id: request.request_id.clone(),
        payload_digest: request.payload_digest().expect("request digest"),
        result: HarnessContextOwnerMutationResult::Draft(HarnessContextOwnerDraftEnvelope {
            schema_version: CONTEXT_OWNER_DRAFT_SCHEMA_V1.to_owned(),
            actor_subject: invocation.identity.console.subject.clone(),
            binding,
            created_at: 100,
            updated_at: 101,
            retention_expires_at: None,
            draft,
        }),
        created_at: 101,
    };
    serde_json::to_vec(&receipt).expect("receipt JSON")
}

fn admission(
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
) -> TrustedInvocationAdmission {
    TrustedInvocationAdmission::synthetic(invocation, use_kind, 1)
}

fn provider() -> TestProvider {
    TestProvider::new()
}

#[test]
fn encrypted_claim_contains_no_plaintext_canary_and_reopens_as_lookup_only() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let marker = "owner-note-canary-987654321-private";
    let call = invocation("request-canary", marker);
    let original = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).expect("open store");
    let ReservationOutcome::Ready(reservation) =
        store.reserve(&call, &original, 10).expect("reserve")
    else {
        panic!("new key should reserve");
    };
    let permit = store
        .claim_send(reservation, &original, 11)
        .expect("durable write claim");
    assert_eq!(permit.body(), call.harness_body().unwrap().unwrap());
    drop(permit); // Simulated crash after claim; state must never enable a second write.
    store.check_storage().expect("bounded files");
    drop(store);

    for suffix in ["", "-wal", "-shm"] {
        let path = if suffix.is_empty() {
            temp.path().to_owned()
        } else {
            PathBuf::from(format!("{}{}", temp.path().display(), suffix))
        };
        if let Ok(bytes) = fs::read(path) {
            assert!(
                !bytes
                    .windows(marker.len())
                    .any(|window| window == marker.as_bytes())
            );
            assert!(
                !bytes
                    .windows(b"user-test".len())
                    .any(|window| window == b"user-test")
            );
        }
    }

    let lookup_auth = admission(&call, AdmissionUse::ExactLookup);
    let mut recovered = OwnerInvocationStore::open(temp.path(), keys).expect("reopen store");
    let ReservationOutcome::LookupRequired(reservation) = recovered
        .reserve(&call, &lookup_auth, 12)
        .expect("read only recovery required")
    else {
        panic!("write claim must require receipt lookup");
    };
    let permit = recovered
        .claim_exact_lookup(reservation, &call, &lookup_auth, 13)
        .expect("claim exact receipt lookup");
    assert!(
        permit
            .endpoint()
            .path
            .ends_with("/context-owner-mutation-receipts/lookup")
    );
    assert!(matches!(
        permit.invocation().operation,
        ContextOwnerOperationV2::LookupMutation { .. }
    ));
    drop(permit);
}

#[test]
fn stable_key_conflict_is_rejected_before_a_second_send_claim() {
    let temp = PrivateTempStore::new();
    let mut store = OwnerInvocationStore::open(temp.path(), provider()).expect("open store");
    let original = invocation("request-stable", "private note A");
    let initial_auth = admission(&original, AdmissionUse::Write);
    let ReservationOutcome::Ready(reservation) =
        store.reserve(&original, &initial_auth, 10).unwrap()
    else {
        panic!("new reservation");
    };
    drop(store.claim_send(reservation, &initial_auth, 11).unwrap());
    let changed = invocation("request-stable", "private note B");
    let changed_auth = admission(&changed, AdmissionUse::ExactLookup);
    assert!(matches!(
        store.reserve(&changed, &changed_auth, 12),
        Err(StoreError::Conflict)
    ));
}

#[test]
fn exact_receipt_completes_and_duplicate_returns_only_encrypted_cache() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let call = invocation("request-complete", "private note for response");
    let auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).expect("open store");
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    let permit = store.claim_send(reservation, &auth, 11).unwrap();
    let response = store
        .complete_send(permit, &receipt_bytes(&call), 12)
        .expect("validated receipt");
    assert!(matches!(response, HarnessResponseV1::MutationReceipt(_)));
    drop(store);

    let refreshed = refreshed_identity(&call);
    let read_auth = admission(&refreshed, AdmissionUse::CachedRead);
    let mut reopened = OwnerInvocationStore::open(temp.path(), keys).expect("reopen cache");
    let ReservationOutcome::Cached(cached) = reopened.reserve(&refreshed, &read_auth, 13).unwrap()
    else {
        panic!("completed exact request should return cache");
    };
    assert!(matches!(*cached, HarnessResponseV1::MutationReceipt(_)));
}

#[test]
fn absent_exact_receipt_stays_unknown_after_restart() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let call = invocation("request-no-receipt", "private note unknown");
    let write_auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &write_auth, 10).unwrap()
    else {
        panic!("new reservation");
    };
    drop(store.claim_send(reservation, &write_auth, 11).unwrap());
    drop(store);

    let read_auth = admission(&call, AdmissionUse::ExactLookup);
    let mut recovered = OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap();
    let ReservationOutcome::LookupRequired(reservation) =
        recovered.reserve(&call, &read_auth, 12).unwrap()
    else {
        panic!("lookup required");
    };
    let permit = recovered
        .claim_exact_lookup(reservation, &call, &read_auth, 13)
        .unwrap();
    assert_eq!(
        recovered.complete_lookup(permit, b"null", 14).unwrap(),
        None
    );
    drop(recovered);

    let mut final_open = OwnerInvocationStore::open(temp.path(), keys).unwrap();
    assert!(matches!(
        final_open.reserve(&call, &read_auth, 15).unwrap(),
        ReservationOutcome::LookupRequired(_)
    ));
}

#[test]
fn missing_data_key_and_ciphertext_tampering_fail_closed() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let call = invocation("request-tamper", "tamper-private-note");
    let auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    drop(store.claim_send(reservation, &auth, 11).unwrap());
    drop(store);

    keys.lose_old_data_key();
    assert_eq!(
        OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap_err(),
        StoreError::KeyUnavailable
    );
    keys.restore_old_data_key();
    let connection = Connection::open(temp.path()).unwrap();
    connection
        .execute(
            "UPDATE owner_invocations SET ciphertext = zeroblob(length(ciphertext))",
            [],
        )
        .unwrap();
    drop(connection);
    assert_eq!(
        OwnerInvocationStore::open(temp.path(), keys).unwrap_err(),
        StoreError::StoreCorrupt
    );
}

#[test]
fn data_key_rotation_reencrypts_all_rows_and_capacity_never_evicts() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap();
    for index in 0..storage::MAX_ROWS {
        let call = invocation(&format!("request-row-{index}"), "bounded test note");
        let auth = admission(&call, AdmissionUse::Write);
        assert!(matches!(
            store.reserve(&call, &auth, 10 + index as u64).unwrap(),
            ReservationOutcome::Ready(_)
        ));
    }
    let full = invocation("request-row-overflow", "must not evict prior records");
    let full_auth = admission(&full, AdmissionUse::Write);
    assert!(matches!(
        store.reserve(&full, &full_auth, 40),
        Err(StoreError::Capacity)
    ));
    drop(store);

    let rotation_temp = PrivateTempStore::new();
    let rotation_keys = provider();
    let call = invocation("request-rotate", "reencrypt private row");
    let auth = admission(&call, AdmissionUse::Write);
    let mut rotating =
        OwnerInvocationStore::open(rotation_temp.path(), rotation_keys.clone()).unwrap();
    let ReservationOutcome::Ready(_) = rotating.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    rotation_keys.rotate_data();
    rotating
        .rotate_data_keys()
        .expect("atomic data-key rotation");
    let row = storage::read_row(
        &rotating.connection,
        &crypto::lookup_tag(
            rotating.keys.index_key(),
            &operations::stable_locator(&call).unwrap(),
        )
        .unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(row.data_key_id, "data-v2");
    drop(rotating);
    OwnerInvocationStore::open(rotation_temp.path(), rotation_keys).expect("reopen rotated store");
}

fn refreshed_identity(original: &ContextOwnerInvocationV2) -> ContextOwnerInvocationV2 {
    let mut current = original.clone();
    current.identity.console.credential_id = "console-credential-v2".to_owned();
    current.identity.console.grant_id = "grant-edit-v2".to_owned();
    current.identity.console.grant_generation = 2;
    current.identity.console.grant_expires_at += 100;
    current.identity.harness.credential_reference_id = "vault-reference-v2".to_owned();
    current
}
