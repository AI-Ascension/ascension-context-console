// SPDX-License-Identifier: MIT

use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const FIXTURE: &[u8] = include_bytes!("../../../../fixtures/synthetic/snapshot.json");
const CLI_FIXTURE: &[u8] = include_bytes!("../../../../fixtures/valid/snapshot-cli.json");
const NOW: SystemTime = UNIX_EPOCH;

fn grant(privilege: CapturePrivilege) -> ReadGrant {
    ReadGrant::issue(
        b"unit-test-token",
        "agent-t02-reader",
        None,
        privilege,
        Duration::from_secs(60),
        NOW,
    )
    .expect("valid grant")
}

#[test]
fn ingest_is_idempotent_but_conflicting_bytes_are_rejected() {
    let mut store = Store::default();
    let summary = store.ingest(FIXTURE).expect("fixture");
    assert_eq!(summary.snapshot_id, "snapshot-t02-synthetic-001");
    assert_eq!(store.ingest(FIXTURE), Ok(summary.clone()));
    let mut changed = FIXTURE.to_vec();
    changed.push(b' ');
    assert!(matches!(store.ingest(&changed), Err(IngestError::Conflict)));
}

#[test]
fn metadata_grant_reads_metadata_fixture_and_scope_is_checked() {
    let mut store = Store::default();
    store.ingest(FIXTURE).expect("fixture");
    let grant = grant(CapturePrivilege::Metadata);
    assert!(
        store
            .get(
                b"unit-test-token",
                &grant,
                "snapshot-t02-synthetic-001",
                NOW
            )
            .is_ok()
    );
    assert!(matches!(
        store.get(b"wrong-token", &grant, "snapshot-t02-synthetic-001", NOW),
        Err(ReadError::Forbidden)
    ));
}

#[test]
fn expiry_and_revoke_are_fail_closed() {
    let mut store = Store::default();
    store.ingest(FIXTURE).expect("fixture");
    let grant = grant(CapturePrivilege::Metadata);
    assert!(matches!(
        store.get(
            b"unit-test-token",
            &grant,
            "snapshot-t02-synthetic-001",
            NOW + Duration::from_secs(60)
        ),
        Err(ReadError::Expired)
    ));
    store.revoke(b"unit-test-token").expect("revoke");
    assert!(matches!(
        store.get(
            b"unit-test-token",
            &grant,
            "snapshot-t02-synthetic-001",
            NOW
        ),
        Err(ReadError::Forbidden)
    ));
}

#[test]
fn content_requires_matching_opaque_blob_and_content_privilege() {
    let mut store = Store::default();
    store.ingest(CLI_FIXTURE).expect("snapshot");
    let bytes = include_bytes!("../../../../fixtures/blobs/fixture-stdin.txt");
    store
        .ingest_content("blob-fixture-stdin", bytes)
        .expect("content");
    let grant = ReadGrant::issue(
        b"content-token",
        "agent-fixture-001",
        Some("run-fixture-001".to_owned()),
        CapturePrivilege::Content,
        Duration::from_secs(60),
        NOW,
    )
    .expect("grant");
    let summary = store
        .component(
            b"content-token",
            &grant,
            "snapshot-fixture-cli-001",
            "component-fixture-stdin",
            NOW,
        )
        .expect("component");
    assert!(summary.content_available);
    assert_eq!(
        store
            .content(
                b"content-token",
                &grant,
                "snapshot-fixture-cli-001",
                "component-fixture-stdin",
                NOW,
            )
            .expect("content"),
        bytes
    );
}

#[test]
fn events_are_filtered_by_project_and_run_scope() {
    let mut store = Store::default();
    store.ingest(FIXTURE).expect("reader snapshot");
    store.ingest(CLI_FIXTURE).expect("fixture snapshot");
    store
        .append_event(
            &serde_json::to_vec(&serde_json::json!({
                "schema":"ascension.context-event.v1",
                "event_id":"event-hidden-sequence",
                "producer_id":"producer-hidden",
                "sequence":1,
                "snapshot_id":"snapshot-t02-synthetic-001",
                "provider_attempt_id":null,
                "observed_at":"2026-09-09T00:00:00Z",
                "event_type":"snapshot.prepared",
                "details":{}
            }))
            .expect("hidden event"),
        )
        .expect("hidden event ingest");
    for line in include_bytes!("../../../../fixtures/valid/events.jsonl")
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        store.append_event(line).expect("event");
    }
    let fixture_grant = ReadGrant::issue(
        b"fixture-token",
        "agent-fixture-001",
        Some("run-fixture-001".to_owned()),
        CapturePrivilege::Metadata,
        Duration::from_secs(60),
        NOW,
    )
    .expect("grant");
    assert_eq!(
        store
            .events(
                b"fixture-token",
                &fixture_grant,
                "run-fixture-001",
                None,
                20,
                NOW,
            )
            .expect("events")
            .events
            .len(),
        7
    );
    let first_page = store
        .events(
            b"fixture-token",
            &fixture_grant,
            "run-fixture-001",
            None,
            2,
            NOW,
        )
        .expect("first page");
    assert_eq!(
        first_page
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(first_page.next_cursor.is_some());
    assert!(!first_page.gap);
    let second_page = store
        .events(
            b"fixture-token",
            &fixture_grant,
            "run-fixture-001",
            first_page.next_cursor.as_deref(),
            2,
            NOW,
        )
        .expect("second page");
    assert_eq!(
        second_page
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert!(!second_page.gap);
    assert_eq!(
        store
            .events(
                b"fixture-token",
                &fixture_grant,
                "run-t02-001",
                None,
                20,
                NOW,
            )
            .expect_err("run-scoped grant cannot read another run"),
        ReadError::NotFound
    );
}

#[test]
fn event_cursor_remains_stable_when_a_late_producer_sequence_arrives() {
    let mut store = Store::default();
    store.ingest(CLI_FIXTURE).expect("fixture snapshot");
    let append = |store: &mut Store, event_id: &str, sequence: u64| {
        store
            .append_event(
                &serde_json::to_vec(&serde_json::json!({
                    "schema":"ascension.context-event.v1",
                    "event_id":event_id,
                    "producer_id":"producer-ordered",
                    "sequence":sequence,
                    "snapshot_id":"snapshot-fixture-cli-001",
                    "provider_attempt_id":null,
                    "observed_at":"2026-09-09T00:00:00Z",
                    "event_type":"snapshot.prepared",
                    "details":{}
                }))
                .expect("event bytes"),
            )
            .expect("event");
    };
    append(&mut store, "event-ordered-0", 0);
    append(&mut store, "event-ordered-2", 2);
    let grant = ReadGrant::issue(
        b"fixture-token",
        "agent-fixture-001",
        Some("run-fixture-001".to_owned()),
        CapturePrivilege::Metadata,
        Duration::from_secs(60),
        NOW,
    )
    .expect("grant");
    let first_page = store
        .events(b"fixture-token", &grant, "run-fixture-001", None, 1, NOW)
        .expect("first page");
    assert_eq!(
        first_page
            .events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["event-ordered-0"]
    );
    assert!(first_page.next_cursor.is_some());

    // The producer's missing sequence arrives after the page was issued. Its immutable
    // scope ordinal follows the already visible event and cannot be skipped by cursor 0.
    append(&mut store, "event-ordered-1", 1);
    let second_page = store
        .events(
            b"fixture-token",
            &grant,
            "run-fixture-001",
            first_page.next_cursor.as_deref(),
            10,
            NOW,
        )
        .expect("second page");
    assert_eq!(
        second_page
            .events
            .iter()
            .map(|event| (event.event_id.as_str(), event.sequence))
            .collect::<Vec<_>>(),
        vec![("event-ordered-2", 1), ("event-ordered-1", 2)]
    );
}

#[test]
fn event_grants_and_cursors_are_bound_to_the_requested_run_stream() {
    let mut store = Store::default();
    store.ingest(CLI_FIXTURE).expect("run A snapshot");
    let run_b = String::from_utf8(CLI_FIXTURE.to_vec())
        .expect("fixture utf8")
        .replace("snapshot-fixture-cli-001", "snapshot-fixture-cli-002")
        .replace("run-fixture-001", "run-fixture-002");
    store.ingest(run_b.as_bytes()).expect("run B snapshot");
    let append = |store: &mut Store, event_id: &str, snapshot_id: &str| {
        store
            .append_event(
                &serde_json::to_vec(&serde_json::json!({
                    "schema":"ascension.context-event.v1",
                    "event_id":event_id,
                    "producer_id":"producer-scoped",
                    "sequence":0,
                    "snapshot_id":snapshot_id,
                    "provider_attempt_id":null,
                    "observed_at":"2026-09-09T00:00:00Z",
                    "event_type":"snapshot.prepared",
                    "details":{}
                }))
                .expect("event bytes"),
            )
            .expect("event");
    };
    append(&mut store, "event-run-a-0", "snapshot-fixture-cli-001");
    append(&mut store, "event-run-a-1", "snapshot-fixture-cli-001");
    append(&mut store, "event-run-b-0", "snapshot-fixture-cli-002");

    let project_grant = ReadGrant::issue(
        b"project-token",
        "agent-fixture-001",
        None,
        CapturePrivilege::Metadata,
        Duration::from_secs(60),
        NOW,
    )
    .expect("project grant");
    let first_page = store
        .events(
            b"project-token",
            &project_grant,
            "run-fixture-001",
            None,
            1,
            NOW,
        )
        .expect("run A page");
    let cursor = first_page.next_cursor.expect("run A cursor");
    assert_eq!(
        store
            .events(
                b"project-token",
                &project_grant,
                "run-fixture-002",
                Some(cursor.as_str()),
                10,
                NOW,
            )
            .expect_err("run A cursor cannot be replayed on run B"),
        ReadError::InvalidScope
    );

    let run_grant = ReadGrant::issue(
        b"run-token",
        "agent-fixture-001",
        Some("run-fixture-001".to_owned()),
        CapturePrivilege::Metadata,
        Duration::from_secs(60),
        NOW,
    )
    .expect("run grant");
    assert_eq!(
        store
            .events(b"run-token", &run_grant, "run-fixture-002", None, 10, NOW,)
            .expect_err("run grant cannot read another run"),
        ReadError::NotFound
    );
}

#[test]
fn out_of_scope_ids_have_the_same_not_found_result_as_unknown_ids() {
    let mut store = Store::default();
    store.ingest(FIXTURE).expect("fixture");
    let grant = grant(CapturePrivilege::Metadata);
    assert_eq!(
        store
            .get(b"unit-test-token", &grant, "snapshot-fixture-cli-001", NOW,)
            .expect_err("other project is hidden"),
        ReadError::NotFound
    );
    assert_eq!(
        store
            .get(b"unit-test-token", &grant, "missing-snapshot", NOW)
            .expect_err("missing snapshot"),
        ReadError::NotFound
    );
}

#[test]
fn plaintext_store_rejects_private_snapshots_with_content_refs() {
    let private = String::from_utf8(CLI_FIXTURE.to_vec())
        .expect("fixture utf8")
        .replace(
            "\"capture_mode\": \"memory\"",
            "\"capture_mode\": \"private\"",
        );
    let mut store = Store::default();
    assert!(matches!(
        store.ingest(private.as_bytes()),
        Err(IngestError::PrivateContentUnsupported)
    ));
}
