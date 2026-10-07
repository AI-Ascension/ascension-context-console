use super::*;
use crate::harness_facade::FacadePermission;
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[test]
fn expired_admission_cannot_claim_a_send_after_reservation() {
    let temp = PrivateTempStore::new();
    let call = invocation("request-expired-admission", "private expiry note");
    let auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), provider()).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    assert!(matches!(
        store.claim_send(reservation, &auth, 2_000_000_000),
        Err(StoreError::Denied)
    ));
}

#[test]
fn admission_expired_during_verification_cannot_be_used() {
    let call = invocation("request-slow-admission", "private slow note");
    let started = Instant::now()
        .checked_sub(Duration::from_secs(61))
        .expect("test clock can model slow verification");
    let auth =
        TrustedInvocationAdmission::synthetic_with_started(&call, AdmissionUse::Write, 1, started);
    assert!(matches!(
        auth.validate_for(&call, AdmissionUse::Write, 10),
        Err(super::super::record::AdmissionError::Denied)
    ));
}

#[test]
fn writer_wait_that_expires_admission_returns_no_send_permit() {
    let temp = PrivateTempStore::new();
    let call = invocation("request-writer-wait", "private writer wait note");
    let long_auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), provider()).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &long_auth, 10).unwrap()
    else {
        panic!("new reservation");
    };

    let lock = Connection::open(temp.path()).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let lock_released = Arc::new(AtomicBool::new(false));
    let release_notice = Arc::clone(&lock_released);
    let release = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1_250));
        lock.execute_batch("ROLLBACK").unwrap();
        release_notice.store(true, Ordering::SeqCst);
    });

    let short_auth = TrustedInvocationAdmission::synthetic_with_lifetime(
        &call,
        AdmissionUse::Write,
        2,
        Duration::from_secs(1),
    );
    let started = Instant::now();
    assert!(matches!(
        store.claim_send(reservation, &short_auth, 20),
        Err(StoreError::Denied)
    ));
    assert!(started.elapsed() >= Duration::from_millis(1_100));
    release.join().unwrap();
    assert!(lock_released.load(Ordering::SeqCst));

    assert!(matches!(
        store.reserve(&call, &long_auth, 21),
        Ok(ReservationOutcome::Ready(_))
    ));
}

#[test]
fn current_admission_cannot_be_reused_with_changed_credential_or_grant_claims() {
    let call = invocation("request-fresh-admission", "private admission note");
    let auth = admission(&call, AdmissionUse::Write);

    let mut changed_credential = call.clone();
    changed_credential.identity.console.credential_id = "console-credential-new".to_owned();
    assert!(super::super::operations::same_call(
        &call,
        &changed_credential
    ));
    assert!(matches!(
        auth.validate_for(&changed_credential, AdmissionUse::Write, 10),
        Err(super::super::record::AdmissionError::Denied)
    ));

    let mut changed_grant = call.clone();
    changed_grant.identity.console.grant_id = "grant-new".to_owned();
    changed_grant.identity.console.grant_generation += 1;
    changed_grant.identity.console.grant_expires_at -= 1;
    assert!(super::super::operations::same_call(&call, &changed_grant));
    assert!(matches!(
        auth.validate_for(&changed_grant, AdmissionUse::Write, 10),
        Err(super::super::record::AdmissionError::Denied)
    ));

    let mut changed_owner_reference = call.clone();
    changed_owner_reference
        .identity
        .harness
        .credential_reference_id = "vault-reference-new".to_owned();
    changed_owner_reference
        .identity
        .harness
        .credential_expires_at -= 1;
    assert!(super::super::operations::same_call(
        &call,
        &changed_owner_reference
    ));
    assert!(matches!(
        auth.validate_for(&changed_owner_reference, AdmissionUse::Write, 10),
        Err(super::super::record::AdmissionError::Denied)
    ));
}

#[test]
fn prepared_reservation_uses_fresh_admitted_identity_after_credential_rotation() {
    let temp = PrivateTempStore::new();
    let original = invocation("request-rotated-admission", "same exact operation");
    let mut refreshed = original.clone();
    refreshed.identity.console.credential_id = "console-credential-rotated".to_owned();
    refreshed.identity.console.grant_id = "grant-edit-rotated".to_owned();
    refreshed.identity.console.grant_generation += 1;
    refreshed.identity.console.grant_expires_at -= 1;
    refreshed.identity.harness.credential_reference_id = "vault-reference-rotated".to_owned();
    refreshed.identity.harness.credential_expires_at -= 1;
    assert!(super::super::operations::same_call(&original, &refreshed));

    let initial_auth = admission(&original, AdmissionUse::Write);
    let refreshed_auth = admission(&refreshed, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), provider()).unwrap();
    assert!(matches!(
        store.reserve(&original, &initial_auth, 10),
        Ok(ReservationOutcome::Ready(_))
    ));
    let ReservationOutcome::Ready(reservation) =
        store.reserve(&refreshed, &refreshed_auth, 11).unwrap()
    else {
        panic!("prepared exact request should allow a newly authorized attempt");
    };
    let permit = store.claim_send(reservation, &refreshed_auth, 12).unwrap();
    assert_eq!(permit.invocation(), &refreshed);
    assert_eq!(
        permit.protected_reference().as_str(),
        "vault-reference-rotated"
    );
}

#[test]
fn authenticated_unknown_persisted_record_fields_fail_closed() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let call = invocation("request-closed-record", "closed versioned record");
    let auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    let tag = reservation.tag;
    let permit = store.claim_send(reservation, &auth, 11).unwrap();
    drop(permit);
    let row = super::super::storage::read_row(&store.connection, &tag)
        .unwrap()
        .unwrap();
    let plaintext = super::super::crypto::decrypt(
        &store.keys,
        store.keys.index_key_id(),
        &row.tag,
        &row.entry_id,
        row.state as i64,
        row.sequence,
        &row.data_key_id,
        &row.nonce,
        &row.ciphertext,
    )
    .unwrap();
    let original: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
    let mut variants = Vec::new();
    let mut unknown_record = original.clone();
    unknown_record
        .as_object_mut()
        .unwrap()
        .insert("future_record_field".to_owned(), serde_json::Value::Null);
    variants.push(unknown_record);
    let mut unknown_admission = original.clone();
    unknown_admission["origin"]
        .as_object_mut()
        .unwrap()
        .insert("future_admission_field".to_owned(), serde_json::Value::Null);
    variants.push(unknown_admission);
    let mut unknown_grant = original.clone();
    unknown_grant["origin"]["grants"][0]
        .as_object_mut()
        .unwrap()
        .insert("future_grant_field".to_owned(), serde_json::Value::Null);
    variants.push(unknown_grant);
    let mut unknown_attempt = original;
    unknown_attempt["attempts"][0]
        .as_object_mut()
        .unwrap()
        .insert("future_attempt_field".to_owned(), serde_json::Value::Null);
    variants.push(unknown_attempt);

    for value in variants {
        let encoded = zeroize::Zeroizing::new(serde_json::to_vec(&value).unwrap());
        let (nonce, ciphertext) = super::super::crypto::encrypt(
            &store.keys,
            &row.tag,
            &row.entry_id,
            row.state as i64,
            row.sequence,
            &encoded,
        )
        .unwrap();
        let tampered = super::super::storage::StoredRow {
            tag: row.tag,
            entry_id: row.entry_id,
            state: row.state,
            sequence: row.sequence,
            data_key_id: row.data_key_id.clone(),
            nonce,
            ciphertext,
        };
        assert!(matches!(
            super::super::record_codec::open_record_with_keys(&store.keys, &tampered),
            Err(StoreError::StoreCorrupt)
        ));
    }
}

#[test]
fn eligible_item_content_reads_require_the_content_scopes() {
    let operation = ContextOwnerOperationV2::EligibleItems {
        workflow_run_id: "run-test".to_owned(),
        query: ContextOwnerItemsQueryV1 {
            draft_id: None,
            include_content: Some(true),
        },
    };
    assert_eq!(
        super::super::operations::required_console_permissions(&operation, AdmissionUse::ReadOnly)
            .unwrap(),
        vec![
            FacadePermission::MetadataRead,
            FacadePermission::ContentRead
        ]
    );
    assert_eq!(
        super::super::operations::required_harness_scopes(&operation, AdmissionUse::ReadOnly)
            .unwrap(),
        vec!["workflow:read", "workflow:context:content:read"]
    );
}

#[test]
fn publication_requires_typed_content_write_and_harness_content_write() {
    let mut call = invocation("request-publication-scope", "note");
    let binding = call.expected_binding.as_ref().unwrap();
    let request = HarnessContextOwnerDraftPublicationRequest {
        schema_version: CONTEXT_OWNER_PUBLICATION_REQUEST_SCHEMA_V1.to_owned(),
        request_id: "publication-request".to_owned(),
        draft_id: "draft-test".to_owned(),
        expected_draft_version: 7,
        expected_owner_state_version: 7,
        expected_base_revision_id: "revision-4".to_owned(),
        expected_binding_id: binding.binding_id.clone(),
        expected_binding_digest: binding.binding_digest.clone(),
        expected_boundary: boundary(),
    };
    call.operation = ContextOwnerOperationV2::PublishDraft {
        workflow_run_id: "run-test".to_owned(),
        draft_id: "draft-test".to_owned(),
        request,
    };
    assert_eq!(
        super::super::operations::required_console_permissions(
            &call.operation,
            AdmissionUse::Write
        )
        .unwrap(),
        vec![FacadePermission::ContentWrite]
    );
    assert_eq!(
        super::super::operations::required_harness_scopes(&call.operation, AdmissionUse::Write)
            .unwrap(),
        vec!["workflow:content:write"]
    );
}

#[test]
fn data_key_id_cannot_be_reused_with_changed_bytes() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let call = invocation("request-key-id-reuse", "same id different bytes");
    let auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    drop(reservation);

    keys.replace_existing_data_key_bytes();
    assert!(matches!(
        store.rotate_data_keys(),
        Err(StoreError::KeyUnavailable)
    ));
    assert!(matches!(
        store.reserve(&call, &auth, 11),
        Ok(ReservationOutcome::Ready(_))
    ));
}

#[test]
fn index_key_id_cannot_be_reused_with_changed_bytes() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    let call = invocation("request-index-key-id-reuse", "changed index bytes");
    let auth = admission(&call, AdmissionUse::Write);
    let mut store = OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap();
    let ReservationOutcome::Ready(reservation) = store.reserve(&call, &auth, 10).unwrap() else {
        panic!("new reservation");
    };
    drop(reservation);
    drop(store);

    keys.replace_index_key_bytes_without_changing_id();
    assert!(matches!(
        OwnerInvocationStore::open(temp.path(), keys),
        Err(StoreError::KeyUnavailable)
    ));
}

#[test]
fn empty_store_reopen_rejects_reused_index_key_id() {
    let temp = PrivateTempStore::new();
    let keys = provider();
    drop(OwnerInvocationStore::open(temp.path(), keys.clone()).unwrap());
    keys.replace_index_key_bytes_without_changing_id();
    assert!(matches!(
        OwnerInvocationStore::open(temp.path(), keys),
        Err(StoreError::KeyUnavailable)
    ));
}

#[test]
fn concurrent_empty_store_open_binds_one_index_key_material() {
    let temp = PrivateTempStore::new();
    let (connection, _) = super::super::storage::open_database(temp.path()).unwrap();
    drop(connection);
    let correct = provider();
    let wrong = provider();
    wrong.replace_index_key_bytes_without_changing_id();
    let start = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for candidate in [correct, wrong] {
        let path = temp.path().to_owned();
        let start = Arc::clone(&start);
        workers.push(std::thread::spawn(move || {
            start.wait();
            OwnerInvocationStore::open(path, candidate).map(drop)
        }));
    }
    start.wait();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().expect("store open worker"))
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(StoreError::KeyUnavailable)))
            .count(),
        1
    );
    let rows: i64 = Connection::open(temp.path())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM owner_invocations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(rows, 0);
}

#[cfg(unix)]
#[test]
fn private_store_rejects_hardlinked_database_and_symlinked_parent() {
    let temp = PrivateTempStore::new();
    drop(OwnerInvocationStore::open(temp.path(), provider()).unwrap());

    let hardlink = temp.directory.join("linked.sqlite3");
    std::fs::hard_link(temp.path(), &hardlink).unwrap();
    assert!(matches!(
        OwnerInvocationStore::open(temp.path(), provider()),
        Err(StoreError::Invalid)
    ));
    std::fs::remove_file(&hardlink).unwrap();

    let linked_directory = temp.directory.join("linked-directory");
    std::os::unix::fs::symlink(&temp.directory, &linked_directory).unwrap();
    let through_link = linked_directory.join("other.sqlite3");
    assert!(matches!(
        OwnerInvocationStore::open(&through_link, provider()),
        Err(StoreError::Invalid)
    ));
    std::fs::remove_file(linked_directory).unwrap();
}

#[cfg(not(unix))]
#[test]
fn private_store_refuses_platform_without_verified_acl_before_file_creation() {
    let temp = PrivateTempStore::new();
    assert!(matches!(
        OwnerInvocationStore::open(temp.path(), provider()),
        Err(StoreError::UnsupportedPlatform)
    ));
    assert!(!temp.path().exists());
}
