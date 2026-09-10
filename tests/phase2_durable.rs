// SPDX-License-Identifier: MIT

#![allow(clippy::expect_used, clippy::unwrap_used)]

use context_service::{
    ControlCommand, ControlOperation, ControlPatch, ControlPlane, DurableControlStore,
    DurableStoreError, DurableStoreFailpoint,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const COMMAND_SCHEMA: &str = "ascension.context-control.command.v1";

fn path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ascension-context-console-target-{label}-{}.sqlite",
        std::process::id()
    ))
}

fn cleanup(path: &PathBuf) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs::remove_file(path.with_extension("sqlite-shm"));
}

fn pause_command(plane: &ControlPlane, key: &str) -> ControlCommand {
    let state = plane.state();
    ControlCommand {
        schema: COMMAND_SCHEMA.to_owned(),
        scope: plane.scope().clone(),
        idempotency_key: key.to_owned(),
        command_window_id: state.command_window_id,
        expected_control_version: state.control_version,
        kind: "pause".to_owned(),
        expected_active_revision_id: None,
        preview_id: None,
        approved_manifest_sha256: None,
        expected_preview_id: None,
    }
}

fn committed_candidate(key_suffix: &str) -> (ControlPlane, ControlPlane) {
    let mut plane = ControlPlane::synthetic();
    let active_revision = plane.state().active_revision_id.clone();
    let operator = format!("operator-{key_suffix}");
    let created = plane
        .create_draft(plane.scope().clone(), &active_revision, &operator)
        .expect("draft");
    let history = plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .expect("history item")
        .item;
    let edited = plane
        .apply_patch(
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: plane.scope().clone(),
                draft_id: created.draft_id.clone(),
                expected_draft_version: created.version,
                expected_active_revision_id: active_revision.clone(),
                operations: vec![ControlOperation::IncludeItem { item: history }],
            },
            &operator,
            false,
        )
        .expect("edit");
    plane
        .pause(pause_command(&plane, &format!("pause-{key_suffix}")))
        .expect("pause");
    let preview = plane
        .create_preview(
            plane.scope().clone(),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("preview");
    let commit = ControlCommand {
        schema: COMMAND_SCHEMA.to_owned(),
        scope: plane.scope().clone(),
        idempotency_key: format!("commit-{key_suffix}"),
        command_window_id: plane.state().command_window_id.clone(),
        expected_control_version: plane.state().control_version,
        kind: "commit".to_owned(),
        expected_active_revision_id: Some(plane.state().active_revision_id.clone()),
        preview_id: Some(preview.preview_id.clone()),
        approved_manifest_sha256: preview.prepared_manifest_sha256.clone(),
        expected_preview_id: Some(preview.preview_id),
    };
    let mut committed = plane.clone();
    committed.commit(commit).expect("commit");
    (plane, committed)
}

#[test]
fn encrypted_journal_reopens_with_phase1_snapshots_and_outbox() {
    let path = path("reopen");
    let key = [7_u8; 32];
    let mut plane = ControlPlane::synthetic();
    let mut store = DurableControlStore::create(&path, key, &plane).expect("create store");
    store
        .copy_phase1_snapshot("snapshot-v1", b"retained Phase 1 bytes")
        .expect("copy snapshot");
    plane
        .pause(pause_command(&plane, "pause-reopen"))
        .expect("pause");
    store.persist(&plane).expect("persist pause");
    let snapshot = store.snapshot().expect("snapshot facts");
    assert!(snapshot.management_active);
    assert_eq!(snapshot.phase1_snapshot_count, 1);
    assert!(!snapshot.phase1_snapshot_digests.is_empty());
    assert!(!snapshot.outbox_event_digests.is_empty());
    let raw = fs::read(&path).expect("read sqlite");
    assert!(
        !raw.windows(b"pause-reopen".len())
            .any(|window| window == b"pause-reopen")
    );
    drop(store);
    let reopened = DurableControlStore::open(&path, key, "fixture-run").expect("reopen");
    let recovered = reopened.load().expect("recover");
    assert!(recovered.state().pause_latched);
    assert_eq!(recovered.state().controller_epoch, 2);
    assert_eq!(
        reopened.snapshot().expect("snapshot").phase1_snapshot_count,
        1
    );
    cleanup(&path);
}

#[test]
fn wrong_key_and_tampering_fail_closed() {
    let database = path("auth");
    let plane = ControlPlane::synthetic();
    DurableControlStore::create(&database, [9_u8; 32], &plane).expect("create");
    let wrong =
        DurableControlStore::open(&database, [8_u8; 32], "fixture-run").expect("open wrong");
    assert_eq!(
        wrong.load().expect_err("wrong key must fail"),
        DurableStoreError::AuthenticationFailed
    );
    assert!(matches!(
        DurableControlStore::open(&database, [0_u8; 32], "fixture-run"),
        Err(DurableStoreError::InvalidKey)
    ));
    {
        let connection = rusqlite::Connection::open(&database).expect("tamper sqlite");
        connection
            .execute(
                "UPDATE context_control_journal SET envelope_digest = '00' WHERE run_id = ?1",
                ["fixture-run"],
            )
            .expect("tamper digest");
    }
    let tampered = DurableControlStore::open(&database, [9_u8; 32], "fixture-run").expect("reopen");
    assert_eq!(
        tampered.load().expect_err("tampered digest must fail"),
        DurableStoreError::Corrupt
    );
    cleanup(&database);
    let mode_path = path("auth-mode");
    DurableControlStore::create(&mode_path, [9_u8; 32], &plane).expect("create mode store");
    {
        let connection = rusqlite::Connection::open(&mode_path).expect("tamper mode sqlite");
        connection
            .execute(
                "UPDATE context_control_journal SET management_active = 0 WHERE run_id = ?1",
                ["fixture-run"],
            )
            .expect("tamper mode");
    }
    let mode_tampered = DurableControlStore::open(&mode_path, [9_u8; 32], "fixture-run")
        .expect("reopen mode tamper");
    assert_eq!(
        mode_tampered
            .load()
            .expect_err("tampered management mode must fail"),
        DurableStoreError::Corrupt
    );
    cleanup(&mode_path);
}

#[test]
fn failed_commit_rolls_back_the_previous_projection() {
    let path = path("rollback");
    let key = [11_u8; 32];
    let plane = ControlPlane::synthetic();
    let mut store = DurableControlStore::create(&path, key, &plane).expect("create");
    let initial_outbox = store
        .snapshot()
        .expect("initial snapshot")
        .outbox_event_count;
    let mut paused = plane.clone();
    paused
        .pause(pause_command(&paused, "pause-failpoint"))
        .expect("pause");
    store.set_failpoint(Some(DurableStoreFailpoint::BeforeCommit));
    assert_eq!(
        store.persist(&paused).expect_err("failpoint must abort"),
        DurableStoreError::Failpoint
    );
    assert!(!store.load().expect("old projection").state().pause_latched);
    assert_eq!(
        store
            .snapshot()
            .expect("rolled back snapshot")
            .outbox_event_count,
        initial_outbox,
        "an aborted transaction must not leave an orphaned outbox event"
    );
    store.persist(&paused).expect("retry");
    assert!(store.load().expect("new projection").state().pause_latched);
    cleanup(&path);
}

#[test]
fn commit_transaction_disk_full_failpoint_preserves_old_revision() {
    let path = path("disk-full");
    let key = [23_u8; 32];
    let (paused, committed) = committed_candidate("disk-full");
    let mut store = DurableControlStore::create(&path, key, &paused).expect("create");
    let before = store.snapshot().expect("before snapshot");
    let old_state = store.load_for_operator().expect("old state").state();
    store.set_failpoint(Some(DurableStoreFailpoint::DiskFull));
    assert_eq!(
        store.persist(&committed).expect_err("disk-full failpoint"),
        DurableStoreError::Failpoint
    );
    let recovered = store.load_for_operator().expect("old projection");
    assert_eq!(
        recovered.state().active_revision_id,
        old_state.active_revision_id
    );
    assert_eq!(recovered.state().plan_epoch, old_state.plan_epoch);
    assert_eq!(
        store.snapshot().expect("after snapshot").outbox_event_count,
        before.outbox_event_count
    );
    cleanup(&path);
}

#[test]
fn audit_append_failure_rolls_back_journal_and_outbox() {
    let path = path("audit-fail");
    let key = [24_u8; 32];
    let (paused, committed) = committed_candidate("audit-fail");
    let mut store = DurableControlStore::create(&path, key, &paused).expect("create");
    let before = store.snapshot().expect("before snapshot");
    store.set_failpoint(Some(DurableStoreFailpoint::BeforeOutbox));
    assert_eq!(
        store
            .persist(&committed)
            .expect_err("audit append failpoint"),
        DurableStoreError::Failpoint
    );
    let recovered = store.load_for_operator().expect("old projection");
    assert_eq!(
        recovered.state().active_revision_id,
        paused.state().active_revision_id
    );
    assert_eq!(
        store.snapshot().expect("after snapshot").outbox_event_count,
        before.outbox_event_count
    );
    cleanup(&path);
}

#[test]
fn crash_after_commit_before_outbox_publication_retains_revision_and_deduplicates() {
    let path = path("post-commit");
    let key = [25_u8; 32];
    let (paused, committed) = committed_candidate("post-commit");
    let mut store = DurableControlStore::create(&path, key, &paused).expect("create");
    store.set_failpoint(Some(DurableStoreFailpoint::AfterCommitBeforePublication));
    assert_eq!(
        store
            .persist(&committed)
            .expect_err("lost publication acknowledgement"),
        DurableStoreError::Failpoint
    );
    drop(store);
    let mut reopened = DurableControlStore::open(&path, key, "fixture-run").expect("reopen");
    let recovered = reopened
        .load_for_operator()
        .expect("committed journal survives lost publication");
    assert_eq!(
        recovered.state().active_revision_id,
        committed.state().active_revision_id
    );
    let outbox_before = reopened
        .snapshot()
        .expect("outbox before replay")
        .outbox_event_count;
    reopened
        .persist(&committed)
        .expect("replay exact durable projection");
    assert_eq!(
        reopened
            .snapshot()
            .expect("outbox after replay")
            .outbox_event_count,
        outbox_before,
        "replaying a committed projection must not duplicate outbox rows"
    );
    cleanup(&path);
}

#[test]
fn watchdog_restart_preserves_a_healthy_pause_without_auto_resume() {
    let path = path("watchdog-pause");
    let key = [26_u8; 32];
    let mut plane = ControlPlane::synthetic();
    plane
        .pause(pause_command(&plane, "pause-watchdog"))
        .expect("pause");
    let store = DurableControlStore::create(&path, key, &plane).expect("create");
    drop(store);
    let reopened = DurableControlStore::open(&path, key, "fixture-run").expect("reopen");
    let recovered = reopened.load().expect("watchdog restart recovery");
    assert!(recovered.state().pause_latched);
    assert_eq!(recovered.state().status, "paused_ready");
    assert!(
        recovered
            .events()
            .iter()
            .all(|event| event.event_type != "input.submitted")
    );
    cleanup(&path);
}

#[test]
fn dropped_outbox_event_is_rebuilt_from_authoritative_journal() {
    let path = path("outbox-reconcile");
    let key = [27_u8; 32];
    let mut plane = ControlPlane::synthetic();
    plane
        .pause(pause_command(&plane, "pause-outbox-reconcile"))
        .expect("pause");
    let store = DurableControlStore::create(&path, key, &plane).expect("create");
    let expected_count = store.snapshot().expect("snapshot").outbox_event_count;
    drop(store);
    {
        let connection = rusqlite::Connection::open(&path).expect("open sqlite");
        connection
            .execute(
                "DELETE FROM context_control_outbox
                 WHERE run_id = ?1 AND sequence = (
                     SELECT MIN(sequence) FROM context_control_outbox WHERE run_id = ?1
                 )",
                ["fixture-run"],
            )
            .expect("drop one delayed event");
    }
    let mut reopened = DurableControlStore::open(&path, key, "fixture-run").expect("reopen");
    let authoritative = reopened.load_for_operator().expect("authoritative journal");
    reopened
        .persist(&authoritative)
        .expect("reconcile dropped outbox event");
    assert_eq!(
        reopened
            .snapshot()
            .expect("reconciled snapshot")
            .outbox_event_count,
        expected_count
    );
    cleanup(&path);
}

#[test]
fn journal_event_flood_is_rejected_before_recovery() {
    let mut plane = ControlPlane::synthetic();
    plane
        .pause(pause_command(&plane, "pause-event-flood"))
        .expect("pause");
    let mut journal: Value =
        serde_json::from_slice(&plane.export_journal().expect("journal")).expect("json journal");
    let events = journal["plane"]["events"].as_array_mut().expect("events");
    let exemplar = events.first().cloned().expect("pause event");
    events.resize(4097, exemplar);
    let bytes = serde_json::to_vec(&journal).expect("flood journal");
    let error = ControlPlane::recover_journal(&bytes).expect_err("event flood must fail closed");
    assert_eq!(error.code, "journal_invalid");
}

#[test]
fn durable_commit_replay_after_lost_reply_returns_original_receipt() {
    let path = path("lost-reply");
    let key = [19_u8; 32];
    let mut plane = ControlPlane::synthetic();
    let mut store = DurableControlStore::create(&path, key, &plane).expect("create");
    let active_revision = plane.state().active_revision_id.clone();
    let created = plane
        .create_draft(
            plane.scope().clone(),
            &active_revision,
            "operator-lost-reply",
        )
        .expect("draft");
    let history = plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .expect("history item")
        .item;
    let edited = plane
        .apply_patch(
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: plane.scope().clone(),
                draft_id: created.draft_id.clone(),
                expected_draft_version: created.version,
                expected_active_revision_id: active_revision.clone(),
                operations: vec![ControlOperation::IncludeItem { item: history }],
            },
            "operator-lost-reply",
            false,
        )
        .expect("edit");
    let pause = pause_command(&plane, "pause-lost-reply");
    plane.pause(pause).expect("pause");
    let preview = plane
        .create_preview(
            plane.scope().clone(),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("preview");
    store.persist(&plane).expect("persist prepared preview");

    let commit = ControlCommand {
        schema: "ascension.context-control.command.v1".to_owned(),
        scope: plane.scope().clone(),
        idempotency_key: "commit-lost-reply".to_owned(),
        command_window_id: plane.state().command_window_id.clone(),
        expected_control_version: plane.state().control_version,
        kind: "commit".to_owned(),
        expected_active_revision_id: Some(plane.state().active_revision_id.clone()),
        preview_id: Some(preview.preview_id.clone()),
        approved_manifest_sha256: preview.prepared_manifest_sha256.clone(),
        expected_preview_id: Some(preview.preview_id),
    };
    let mut committed = plane.clone();
    let receipt = committed.commit(commit.clone()).expect("commit");
    store.persist(&committed).expect("persist commit intent");
    drop(store);

    let reopened = DurableControlStore::open(&path, key, "fixture-run").expect("reopen");
    let mut recovered = reopened
        .load_for_operator()
        .expect("recover committed journal");
    let replay = recovered.commit(commit).expect("exact command replay");
    assert_eq!(replay, receipt);
    assert_eq!(
        recovered.state().active_revision_id,
        committed.state().active_revision_id
    );
    cleanup(&path);
}

#[test]
fn partial_additive_schema_is_repaired_and_newer_schema_is_rejected() {
    let migration_path = path("migration");
    {
        let connection = rusqlite::Connection::open(&migration_path).expect("raw sqlite");
        connection
            .execute_batch(
                "CREATE TABLE context_control_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
                 INSERT INTO context_control_meta(key, value) VALUES ('schema', 'ascension.context-control.sqlite.v1');
                 INSERT INTO context_control_meta(key, value) VALUES ('schema_version', '1');
                 CREATE TABLE context_control_phase1_snapshots (
                     run_id TEXT NOT NULL,
                     snapshot_id TEXT NOT NULL,
                     snapshot BLOB NOT NULL,
                     digest TEXT NOT NULL,
                     PRIMARY KEY (run_id, snapshot_id)
                 );
                 INSERT INTO context_control_phase1_snapshots
                     (run_id, snapshot_id, snapshot, digest)
                 VALUES ('fixture-run', 'legacy-snapshot', X'6c65676163792050686173652031206279746573',
                         '972999d7f17c30172ee8855045106c73d2ee765e94da85fa5da7074d14a48623');",
            )
            .expect("partial marker");
    }
    let store =
        DurableControlStore::open(&migration_path, [13_u8; 32], "fixture-run").expect("repair");
    assert_eq!(
        store.snapshot().expect_err("journal not invented"),
        DurableStoreError::Missing
    );
    drop(store);
    let connection = rusqlite::Connection::open(&migration_path).expect("reopen migrated sqlite");
    assert_eq!(
        connection
            .query_row(
                "SELECT snapshot FROM context_control_phase1_snapshots
                 WHERE run_id = 'fixture-run' AND snapshot_id = 'legacy-snapshot'",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .expect("legacy snapshot"),
        b"legacy Phase 1 bytes"
    );
    drop(connection);
    cleanup(&migration_path);

    let newer = path("newer");
    {
        let connection = rusqlite::Connection::open(&newer).expect("raw newer sqlite");
        connection
            .execute_batch(
                "CREATE TABLE context_control_meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
                 INSERT INTO context_control_meta(key, value) VALUES ('schema', 'ascension.context-control.sqlite.v1');
                 INSERT INTO context_control_meta(key, value) VALUES ('schema_version', '2');",
            )
            .expect("newer marker");
    }
    assert!(matches!(
        DurableControlStore::open(&newer, [13_u8; 32], "fixture-run"),
        Err(DurableStoreError::Incompatible)
    ));
    cleanup(&newer);
}

#[test]
fn legacy_open_refuses_management_active_and_allows_disabled_state() {
    let path = path("legacy");
    let key = [17_u8; 32];
    let mut plane = ControlPlane::synthetic();
    let mut store = DurableControlStore::create(&path, key, &plane).expect("create");
    assert_eq!(
        DurableControlStore::legacy_open(&path),
        Err(DurableStoreError::ManagementActive)
    );
    plane.deactivate();
    store.persist(&plane).expect("persist disabled state");
    assert_eq!(DurableControlStore::legacy_open(&path), Ok(()));
    assert!(!store.management_active().expect("mode"));
    cleanup(&path);
}

#[test]
fn phase1_snapshot_identity_is_immutable_and_backup_reopens() {
    let database = path("backup");
    let backup = path("backup-copy");
    let plane = ControlPlane::synthetic();
    let mut store = DurableControlStore::create(&database, [19_u8; 32], &plane).expect("create");
    store
        .copy_phase1_snapshot("snapshot-v1", b"immutable Phase 1 bytes")
        .expect("snapshot");
    assert_eq!(
        store
            .copy_phase1_snapshot("snapshot-v1", b"different bytes")
            .expect_err("overwrite must fail"),
        DurableStoreError::SnapshotConflict
    );
    store.backup(&backup).expect("backup");
    let reopened =
        DurableControlStore::open(&backup, [19_u8; 32], "fixture-run").expect("open backup");
    assert_eq!(
        reopened
            .snapshot()
            .expect("backup facts")
            .phase1_snapshot_count,
        1
    );
    cleanup(&database);
    cleanup(&backup);
}
