// SPDX-License-Identifier: MIT

mod support;

use context_service::{
    CapturePrivilege, IngestError, MAX_SNAPSHOTS, ReadError, ReadGrant, Store, StoreConfig,
};
use std::time::{Duration, UNIX_EPOCH};
use support::fixtures::read_fixture;

const NOW: std::time::SystemTime = UNIX_EPOCH;

fn grant(token: &[u8], project: &str, run: Option<&str>, privilege: CapturePrivilege) -> ReadGrant {
    ReadGrant::issue(
        token,
        project,
        run.map(str::to_owned),
        privilege,
        Duration::from_secs(60),
        NOW,
    )
    .expect("valid grant")
}

#[test]
fn ingest_is_idempotent_and_conflicting_bytes_are_rejected() {
    let synthetic = read_fixture("fixtures/synthetic/snapshot.json");
    let mut store = Store::default();
    assert!(store.is_empty());

    let summary = store.ingest(&synthetic).expect("synthetic ingest");
    assert_eq!(summary.snapshot_id, "snapshot-t02-synthetic-001");
    assert_eq!(summary.run_id, "run-t02-001");
    assert_eq!(summary.episode_id, "episode-t02-001");
    assert_eq!(summary.boundary, "adapter.http_body");
    assert_eq!(summary.capture_mode, "metadata");
    assert_eq!(summary.component_count, 1);
    assert!(!summary.application_capture_complete);
    assert_eq!(store.len(), 1);

    assert_eq!(store.ingest(&synthetic).expect("re-ingest"), summary);
    assert_eq!(store.len(), 1);

    let mut changed = synthetic.clone();
    changed.push(b'\n');
    assert_eq!(store.ingest(&changed), Err(IngestError::Conflict));
    assert_eq!(store.ingest(b""), Err(IngestError::Empty));
}

#[test]
fn reads_require_the_matching_token_scope_and_privilege() {
    let synthetic = read_fixture("fixtures/synthetic/snapshot.json");
    let mut store = Store::default();
    store.ingest(&synthetic).expect("ingest");

    let scoped = grant(
        b"store-token",
        "agent-t02-reader",
        Some("run-t02-001"),
        CapturePrivilege::Metadata,
    );
    assert_eq!(scoped.privilege(), CapturePrivilege::Metadata);
    assert_eq!(scoped.project(), "agent-t02-reader");
    assert_eq!(scoped.run(), Some("run-t02-001"));
    assert!(
        store
            .get(b"store-token", &scoped, "snapshot-t02-synthetic-001", NOW)
            .expect("scoped read")
            == synthetic
    );

    assert_eq!(
        store.get(b"wrong-token", &scoped, "snapshot-t02-synthetic-001", NOW),
        Err(ReadError::Forbidden)
    );
    assert_eq!(
        store.get(b"store-token", &scoped, "snapshot-does-not-exist", NOW),
        Err(ReadError::NotFound)
    );

    let foreign_project = grant(
        b"store-token",
        "agent-other",
        None,
        CapturePrivilege::Metadata,
    );
    assert_eq!(
        store.get(
            b"store-token",
            &foreign_project,
            "snapshot-t02-synthetic-001",
            NOW
        ),
        Err(ReadError::NotFound)
    );

    assert_eq!(
        store.get(
            b"store-token",
            &scoped,
            "snapshot-t02-synthetic-001",
            NOW + Duration::from_secs(60)
        ),
        Err(ReadError::Expired)
    );
}

#[test]
fn revoke_fails_closed_for_a_still_valid_grant() {
    let mut store = Store::default();
    store
        .ingest(&read_fixture("fixtures/synthetic/snapshot.json"))
        .expect("ingest");
    let scoped = grant(
        b"revoke-token",
        "agent-t02-reader",
        None,
        CapturePrivilege::Metadata,
    );
    store.revoke(b"revoke-token").expect("revoke");
    assert_eq!(
        store.get(b"revoke-token", &scoped, "snapshot-t02-synthetic-001", NOW),
        Err(ReadError::Forbidden)
    );
    assert_eq!(store.revoke(b""), Err(ReadError::InvalidToken));
}

#[test]
fn compare_reports_boundary_and_component_differences() {
    let mut store = Store::default();
    store
        .ingest(&read_fixture("fixtures/valid/snapshot-cli.json"))
        .expect("cli snapshot");
    store
        .ingest(&read_fixture("fixtures/valid/snapshot-http.json"))
        .expect("http snapshot");

    let scoped = grant(
        b"compare-token",
        "agent-fixture-001",
        Some("run-fixture-001"),
        CapturePrivilege::Content,
    );
    let result = store
        .compare(
            b"compare-token",
            &scoped,
            "snapshot-fixture-cli-001",
            "snapshot-fixture-http-001",
            NOW,
        )
        .expect("compare");
    assert_eq!(result.left_snapshot_id, "snapshot-fixture-cli-001");
    assert_eq!(result.right_snapshot_id, "snapshot-fixture-http-001");
    assert!(!result.same_boundary);
    assert!(!result.same_component_order);
    assert_eq!(
        result.changed_components,
        vec![
            "component-fixture-stdin".to_owned(),
            "component-fixture-schema".to_owned(),
            "component-fixture-config".to_owned(),
        ]
    );

    let same = store
        .compare(
            b"compare-token",
            &scoped,
            "snapshot-fixture-cli-001",
            "snapshot-fixture-cli-001",
            NOW,
        )
        .expect("self compare");
    assert!(same.same_boundary);
    assert!(same.same_component_order);
    assert!(same.changed_components.is_empty());

    let for_run = store
        .compare_for_run(
            b"compare-token",
            &scoped,
            "run-fixture-001",
            "snapshot-fixture-cli-001",
            "snapshot-fixture-http-001",
            NOW,
        )
        .expect("compare for run");
    assert_eq!(for_run, result);
    assert_eq!(
        store.compare_for_run(
            b"compare-token",
            &scoped,
            "run-other",
            "snapshot-fixture-cli-001",
            "snapshot-fixture-http-001",
            NOW
        ),
        Err(ReadError::NotFound)
    );
}

#[test]
fn events_paginate_with_authenticated_cursors() {
    let mut store = Store::default();
    store
        .ingest(&read_fixture("fixtures/valid/snapshot-cli.json"))
        .expect("snapshot");
    let events = read_fixture("fixtures/valid/events.jsonl");
    let lines = events
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 7);
    for line in &lines {
        store.append_event(line).expect("event");
    }
    assert_eq!(store.event_len(), 7);

    store.append_event(lines[0]).expect("idempotent re-append");
    assert_eq!(store.event_len(), 7);

    let mut conflicting = lines[0].to_vec();
    let field = conflicting
        .windows(12)
        .position(|window| window == b"\"sequence\":0")
        .expect("sequence field");
    conflicting[field + 11] = b'1';
    assert_eq!(
        store.append_event(&conflicting),
        Err(IngestError::EventConflict)
    );

    let unknown = serde_json::to_vec(&serde_json::json!({
        "schema": "ascension.context-event.v1",
        "event_id": "event-unknown-snapshot",
        "producer_id": "producer-fixture-001",
        "sequence": 0,
        "snapshot_id": "snapshot-does-not-exist",
        "provider_attempt_id": null,
        "observed_at": "2026-09-09T00:00:00Z",
        "event_type": "snapshot.prepared",
        "details": {}
    }))
    .expect("json");
    assert_eq!(
        store.append_event(&unknown),
        Err(IngestError::InvalidEvent("unknown snapshot_id".to_owned()))
    );

    let scoped = grant(
        b"event-token",
        "agent-fixture-001",
        Some("run-fixture-001"),
        CapturePrivilege::Metadata,
    );
    let first = store
        .events(b"event-token", &scoped, "run-fixture-001", None, 3, NOW)
        .expect("first page");
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(!first.gap);
    let cursor = first.next_cursor.expect("cursor");

    let second = store
        .events(
            b"event-token",
            &scoped,
            "run-fixture-001",
            Some(&cursor),
            3,
            NOW,
        )
        .expect("second page");
    assert_eq!(
        second
            .events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![3, 4, 5]
    );
    let second_cursor = second.next_cursor.expect("cursor");

    let last = store
        .events(
            b"event-token",
            &scoped,
            "run-fixture-001",
            Some(&second_cursor),
            3,
            NOW,
        )
        .expect("last page");
    assert_eq!(
        last.events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![6]
    );
    assert!(last.next_cursor.is_none());

    assert_eq!(
        store.events(
            b"event-token",
            &scoped,
            "run-fixture-001",
            Some("c1-0-deadbeef"),
            3,
            NOW
        ),
        Err(ReadError::InvalidScope)
    );

    let other_token = grant(
        b"other-event-token",
        "agent-fixture-001",
        Some("run-fixture-001"),
        CapturePrivilege::Metadata,
    );
    assert_eq!(
        store.events(
            b"other-event-token",
            &other_token,
            "run-fixture-001",
            Some(&cursor),
            3,
            NOW
        ),
        Err(ReadError::InvalidScope)
    );
}

#[test]
fn content_requires_content_privilege_and_a_matching_digest() {
    let mut store = Store::default();
    store
        .ingest(&read_fixture("fixtures/valid/snapshot-cli.json"))
        .expect("snapshot");
    let blob = read_fixture("fixtures/blobs/fixture-stdin.txt");
    store
        .ingest_content("blob-fixture-stdin", &blob)
        .expect("content");
    assert_eq!(store.content_len(), 1);

    let content_grant = grant(
        b"content-token",
        "agent-fixture-001",
        Some("run-fixture-001"),
        CapturePrivilege::Content,
    );
    let summary = store
        .component(
            b"content-token",
            &content_grant,
            "snapshot-fixture-cli-001",
            "component-fixture-stdin",
            NOW,
        )
        .expect("component");
    assert!(summary.content_available);
    assert!(summary.digest_present);
    assert!(
        store
            .content(
                b"content-token",
                &content_grant,
                "snapshot-fixture-cli-001",
                "component-fixture-stdin",
                NOW
            )
            .expect("content")
            == blob
    );

    let metadata_grant = grant(
        b"content-token",
        "agent-fixture-001",
        Some("run-fixture-001"),
        CapturePrivilege::Metadata,
    );
    assert_eq!(
        store.content(
            b"content-token",
            &metadata_grant,
            "snapshot-fixture-cli-001",
            "component-fixture-stdin",
            NOW
        ),
        Err(ReadError::Forbidden)
    );
    assert_eq!(
        store.get(
            b"content-token",
            &metadata_grant,
            "snapshot-fixture-cli-001",
            NOW
        ),
        Err(ReadError::Forbidden)
    );

    let mut other = blob.clone();
    other.push(b'x');
    assert_eq!(
        store.ingest_content("blob-fixture-stdin", &other),
        Err(IngestError::ContentConflict)
    );

    store
        .ingest_content("blob-fixture-schema", b"not the schema blob")
        .expect("content");
    let schema = store
        .component(
            b"content-token",
            &content_grant,
            "snapshot-fixture-cli-001",
            "component-fixture-schema",
            NOW,
        )
        .expect("component");
    assert!(!schema.content_available);
    assert_eq!(
        store.content(
            b"content-token",
            &content_grant,
            "snapshot-fixture-cli-001",
            "component-fixture-schema",
            NOW
        ),
        Err(ReadError::NotFound)
    );
}

#[test]
fn store_config_bounds_are_enforced() {
    assert_eq!(
        Store::with_config(StoreConfig {
            max_snapshots: 0,
            ..StoreConfig::default()
        })
        .err(),
        Some(IngestError::Capacity)
    );
    assert_eq!(
        Store::with_config(StoreConfig {
            max_manifest_bytes: 0,
            ..StoreConfig::default()
        })
        .err(),
        Some(IngestError::Capacity)
    );
    assert_eq!(
        Store::with_config(StoreConfig {
            max_compare_bytes: 0,
            ..StoreConfig::default()
        })
        .err(),
        Some(IngestError::Capacity)
    );
    assert_eq!(
        Store::with_config(StoreConfig {
            max_snapshots: MAX_SNAPSHOTS + 1,
            ..StoreConfig::default()
        })
        .err(),
        Some(IngestError::Capacity)
    );

    let mut bounded = Store::with_config(StoreConfig {
        max_snapshots: 1,
        ..StoreConfig::default()
    })
    .expect("config");
    bounded
        .ingest(&read_fixture("fixtures/synthetic/snapshot.json"))
        .expect("first");
    assert_eq!(
        bounded.ingest(&read_fixture("fixtures/valid/snapshot-http.json")),
        Err(IngestError::Capacity)
    );

    let mut small = Store::with_config(StoreConfig {
        max_manifest_bytes: 16,
        ..StoreConfig::default()
    })
    .expect("config");
    assert_eq!(
        small.ingest(&read_fixture("fixtures/synthetic/snapshot.json")),
        Err(IngestError::TooLarge)
    );
}
