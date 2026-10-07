use super::tests::{TestRedeemer, credential, serve_once, source_status_invocation, transport_for};
use super::{
    HarnessTransportError, InvocationSendContext, LiveCurrentnessError, LiveInvocationCurrentness,
};
use crate::harness_context_owner_wire::*;
use crate::owner_invocation_store::{
    AdmissionUse, OwnerInvocationKeyMaterial, OwnerInvocationKeyProvider, OwnerInvocationStore,
    ReservationOutcome, StoreError, TrustedInvocationAdmission,
};
use crate::protected_owner_credentials::ResolvedOwnerCredential;
use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct TestKeys;

impl OwnerInvocationKeyProvider for TestKeys {
    fn load(&mut self) -> Result<OwnerInvocationKeyMaterial, StoreError> {
        OwnerInvocationKeyMaterial::new(
            "currentness-index-v1".to_owned(),
            [0x11; 32],
            "currentness-data-v1".to_owned(),
            BTreeMap::from([("currentness-data-v1".to_owned(), [0x22; 32])]),
        )
    }
}

struct PrivateDatabase {
    path: PathBuf,
    _fixture: crate::owner_test_fixtures::PrivateTestDirectory,
}

impl PrivateDatabase {
    fn new() -> Self {
        let fixture = crate::owner_test_fixtures::PrivateTestDirectory::new("currentness");
        let path = fixture.path().join("intent.sqlite3");
        Self {
            path,
            _fixture: fixture,
        }
    }

    fn open(&self) -> OwnerInvocationStore<TestKeys> {
        OwnerInvocationStore::open(&self.path, TestKeys).expect("test store")
    }
}

impl Drop for PrivateDatabase {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let path = if suffix.is_empty() {
                self.path.clone()
            } else {
                PathBuf::from(format!("{}{}", self.path.display(), suffix))
            };
            let _ = fs::remove_file(path);
        }
    }
}

struct RevokeAt {
    calls: usize,
    deny_at: usize,
    expected_use: AdmissionUse,
}

impl RevokeAt {
    fn new(deny_at: usize, expected_use: AdmissionUse) -> Self {
        Self {
            calls: 0,
            deny_at,
            expected_use,
        }
    }
}

impl LiveInvocationCurrentness for RevokeAt {
    fn revalidate(
        &mut self,
        admission: &TrustedInvocationAdmission,
        invocation: &ContextOwnerInvocationV2,
        credential: &ResolvedOwnerCredential,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<(), LiveCurrentnessError> {
        self.calls += 1;
        assert_eq!(use_kind, self.expected_use);
        assert_eq!(
            credential.reference().as_str(),
            invocation.identity.harness.credential_reference_id
        );
        admission
            .validate_for(invocation, use_kind, now)
            .expect("transport retains the exact admitted pair");
        if self.calls == self.deny_at {
            Err(LiveCurrentnessError::Denied)
        } else {
            Ok(())
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_secs()
}

fn live_write_invocation(now: u64) -> ContextOwnerInvocationV2 {
    let mut invocation = source_status_invocation();
    invocation.identity.console.grant_expires_at = now + 600;
    invocation.identity.harness.credential_expires_at = now + 600;
    let boundary = boundary();
    let binding = ContextOwnerBinding {
        schema_version: CONTEXT_OWNER_BINDING_SCHEMA_V1.to_owned(),
        owner_id: "owner-1".to_owned(),
        owner_version: "owner.v1".to_owned(),
        invocation_id: "invocation-1".to_owned(),
        binding_id: "binding-1".to_owned(),
        binding_version: 1,
        binding_digest: "a".repeat(64),
        context_ref: "context-1".to_owned(),
        instance_id: "instance-1".to_owned(),
        node_kind: "context-owner".to_owned(),
        state: ContextBindingState::Available,
        workflow_run_id: "run-1".to_owned(),
        definition_digest: "b".repeat(64),
        graph_id: "graph-1".to_owned(),
        node_id: "node-1".to_owned(),
        node_execution_id: "execution-1".to_owned(),
        boundary: boundary.clone(),
        lease_epoch: 1,
        snapshot_id: "snapshot-1".to_owned(),
        approved_revision_id: "revision-1".to_owned(),
        plan_epoch: 1,
        grants: ContextBindingGrants {
            metadata_read: true,
            content_read: false,
            edit: true,
            control: false,
        },
        continuity: ContextBindingContinuity {
            survives_controller_restart: false,
            receipt_recovery: true,
            provider_session_continuity: false,
        },
    };
    invocation.expected_binding = Some(binding);
    invocation.operation = ContextOwnerOperationV2::PatchDraft {
        workflow_run_id: "run-1".to_owned(),
        draft_id: "draft-1".to_owned(),
        request: HarnessContextOwnerDraftPatchRequest {
            schema_version: CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1.to_owned(),
            request_id: "request-currentness".to_owned(),
            draft_id: "draft-1".to_owned(),
            expected_version: 7,
            expected_boundary: boundary,
            operations: vec![HarnessContextOwnerDraftOperation::PutNote {
                note_id: "note-1".to_owned(),
                text: "private exact request".to_owned(),
            }],
        },
    };
    invocation.validate().expect("typed write invocation");
    invocation
}

fn boundary() -> ContextBoundary {
    ContextBoundary {
        run_id: "run-1".to_owned(),
        episode_id: "episode-1".to_owned(),
        agent_id: "agent-1".to_owned(),
        state_id: "state-1".to_owned(),
        generation: 1,
        observation_sha256: "1".repeat(64),
        catalog_sha256: "2".repeat(64),
        adapter_revision: "adapter.v1".to_owned(),
        model_revision: "model.v1".to_owned(),
        configuration_sha256: "3".repeat(64),
        output_schema_sha256: "4".repeat(64),
        controller_epoch: 1,
        gate_epoch: 1,
        control_version: 9,
    }
}

fn mutation_receipt(invocation: &ContextOwnerInvocationV2) -> Vec<u8> {
    let ContextOwnerOperationV2::PatchDraft { request, .. } = &invocation.operation else {
        panic!("patch invocation fixture required");
    };
    let binding = invocation.expected_binding.clone().expect("binding");
    serde_json::to_vec(&HarnessContextOwnerMutationReceipt {
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
            binding: binding.clone(),
            created_at: 100,
            updated_at: 101,
            retention_expires_at: None,
            draft: ContextDraft {
                schema: "ascension.context-control.draft.v1".to_owned(),
                draft_id: request.draft_id.clone(),
                version: request.expected_version + 1,
                base_revision_id: binding.approved_revision_id.clone(),
                selected_items: Vec::new(),
                pinned_item_ids: Vec::new(),
                notes: Vec::new(),
                objective: None,
                author_ref: invocation.identity.console.subject.clone(),
            },
        }),
        created_at: 101,
    })
    .expect("typed Harness receipt")
}

fn no_socket(listener: &TcpListener) {
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[cfg(unix)]
#[test]
fn revoked_write_before_claim_stays_prepared_and_preconnect_revocation_never_resends() {
    let now = unix_now();
    let invocation = live_write_invocation(now);
    let admission = TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::Write, now);
    let credential = credential(&invocation, &["workflow:context:edit"], now + 600);
    let database = PrivateDatabase::new();
    let mut store = database.open();
    let ReservationOutcome::Ready(reservation) =
        store.reserve(&invocation, &admission, now).unwrap()
    else {
        panic!("prepared exact write");
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let transport = transport_for(listener.local_addr().unwrap(), Duration::from_secs(1));
    let mut redeemer = TestRedeemer::default();
    let mut before_claim = RevokeAt::new(1, AdmissionUse::Write);
    assert_eq!(
        transport.send_reserved_write(
            &mut store,
            reservation,
            &InvocationSendContext::borrowed(
                &invocation,
                &admission,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut before_claim,
        ),
        Err(HarnessTransportError::CredentialDenied)
    );
    no_socket(&listener);
    assert!(matches!(
        store.reserve(&invocation, &admission, now + 1),
        Ok(ReservationOutcome::Ready(_))
    ));

    let now = unix_now();
    let admission = TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::Write, now);
    let ReservationOutcome::Ready(reservation) =
        store.reserve(&invocation, &admission, now).unwrap()
    else {
        panic!("prepared exact write");
    };
    let mut after_claim = RevokeAt::new(2, AdmissionUse::Write);
    assert_eq!(
        transport.send_reserved_write(
            &mut store,
            reservation,
            &InvocationSendContext::borrowed(
                &invocation,
                &admission,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut after_claim,
        ),
        Err(HarnessTransportError::CredentialDenied)
    );
    no_socket(&listener);
    let lookup =
        TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::ExactLookup, now + 1);
    assert!(matches!(
        store.reserve(&invocation, &lookup, now + 1),
        Ok(ReservationOutcome::LookupRequired(_))
    ));
    assert!(!matches!(
        store.reserve(&invocation, &admission, now + 1),
        Ok(ReservationOutcome::Ready(_))
    ));
}

#[cfg(unix)]
#[test]
fn valid_write_receipt_is_cached_before_revoked_result_is_withheld() {
    let now = unix_now();
    let invocation = live_write_invocation(now);
    let admission = TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::Write, now);
    let credential = credential(&invocation, &["workflow:context:edit"], now + 600);
    let database = PrivateDatabase::new();
    let mut store = database.open();
    let ReservationOutcome::Ready(reservation) =
        store.reserve(&invocation, &admission, now).unwrap()
    else {
        panic!("prepared exact write");
    };
    let (address, received) = serve_once(mutation_receipt(&invocation), Duration::ZERO);
    let transport = transport_for(address, Duration::from_secs(1));
    let mut redeemer = TestRedeemer::default();
    let mut currentness = RevokeAt::new(3, AdmissionUse::Write);
    assert_eq!(
        transport.send_reserved_write(
            &mut store,
            reservation,
            &InvocationSendContext::borrowed(
                &invocation,
                &admission,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut currentness,
        ),
        Err(HarnessTransportError::CredentialDenied)
    );
    assert!(
        String::from_utf8(received.join().unwrap())
            .unwrap()
            .starts_with("PATCH ")
    );
    assert_eq!(currentness.calls, 3);
    let cached =
        TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::CachedRead, now + 1);
    assert!(matches!(
        store.reserve(&invocation, &cached, now + 1),
        Ok(ReservationOutcome::Cached(
            response,
        ))
        if matches!(response.as_ref(), HarnessResponseV1::MutationReceipt(_))
    ));
}

#[cfg(unix)]
#[test]
fn exact_lookup_caches_valid_receipt_before_revoked_result_is_withheld() {
    let now = unix_now();
    let invocation = live_write_invocation(now);
    let write = TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::Write, now);
    let database = PrivateDatabase::new();
    let mut store = database.open();
    let ReservationOutcome::Ready(reservation) = store.reserve(&invocation, &write, now).unwrap()
    else {
        panic!("prepared exact write");
    };
    let unknown_at = unix_now();
    let permit = store.claim_send(reservation, &write, unknown_at).unwrap();
    store.mark_send_unknown(permit, unknown_at).unwrap();
    let lookup_now = unix_now();
    let lookup =
        TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::ExactLookup, lookup_now);
    let ReservationOutcome::LookupRequired(reservation) =
        store.reserve(&invocation, &lookup, lookup_now).unwrap()
    else {
        panic!("unknown write must require exact lookup");
    };
    let credential = credential(&invocation, &["workflow:read"], now + 600);
    let (address, received) = serve_once(mutation_receipt(&invocation), Duration::ZERO);
    let transport = transport_for(address, Duration::from_secs(1));
    let mut redeemer = TestRedeemer::default();
    let mut currentness = RevokeAt::new(3, AdmissionUse::ExactLookup);
    assert_eq!(
        transport.send_reserved_lookup(
            &mut store,
            reservation,
            &InvocationSendContext::borrowed(
                &invocation,
                &lookup,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut currentness,
        ),
        Err(HarnessTransportError::CredentialDenied)
    );
    assert!(
        String::from_utf8(received.join().unwrap())
            .unwrap()
            .starts_with("POST ")
    );
    assert_eq!(currentness.calls, 3);
    let cached =
        TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::CachedRead, now + 4);
    assert!(matches!(
        store.reserve(&invocation, &cached, now + 4),
        Ok(ReservationOutcome::Cached(
            response,
        ))
        if matches!(response.as_ref(), HarnessResponseV1::MutationReceipt(_))
    ));
}

#[cfg(unix)]
#[test]
fn read_currentness_is_checked_at_preconnect_and_after_typed_decode() {
    let now = unix_now();
    let mut invocation = source_status_invocation();
    invocation.identity.console.grant_expires_at = now + 600;
    invocation.identity.harness.credential_expires_at = now + 600;
    let admission = TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::ReadOnly, now);
    let credential = credential(&invocation, &["workflow:read"], now + 600);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let transport = transport_for(listener.local_addr().unwrap(), Duration::from_secs(1));
    let mut redeemer = TestRedeemer::default();
    let mut before_connect = RevokeAt::new(2, AdmissionUse::ReadOnly);
    assert_eq!(
        transport.send_read_once(
            &InvocationSendContext::borrowed(
                &invocation,
                &admission,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut before_connect,
        ),
        Err(HarnessTransportError::CredentialDenied)
    );
    no_socket(&listener);

    let (address, received) = serve_once(source_status_bytes(), Duration::ZERO);
    let transport = transport_for(address, Duration::from_secs(1));
    let mut before_disclosure = RevokeAt::new(3, AdmissionUse::ReadOnly);
    assert_eq!(
        transport.send_read_once(
            &InvocationSendContext::borrowed(
                &invocation,
                &admission,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut before_disclosure,
        ),
        Err(HarnessTransportError::CredentialDenied)
    );
    assert!(
        String::from_utf8(received.join().unwrap())
            .unwrap()
            .starts_with("GET ")
    );
    assert_eq!(before_disclosure.calls, 3);
}

fn source_status_bytes() -> Vec<u8> {
    serde_json::to_vec(&ContextOwnerSourceStatus {
        schema_version: CONTEXT_OWNER_SOURCE_STATUS_SCHEMA_V1.to_owned(),
        owner_id: "owner-1".to_owned(),
        owner_version: "owner.v1".to_owned(),
        workflow_run_id: "run-1".to_owned(),
        definition_digest: "b".repeat(64),
        instance_id: "instance-1".to_owned(),
        boundary: boundary(),
        active_revision_id: "revision-1".to_owned(),
        active_source: None,
    })
    .expect("typed source status")
}
