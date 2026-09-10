// SPDX-License-Identifier: MIT

use context_reader::{CaptureMode, ComponentStatus, parse_event_lines, parse_snapshot};
use context_service::{CapturePrivilege, ReadGrant, Store, TransportState};
use std::time::{Duration, UNIX_EPOCH};

#[test]
fn contract_fixtures_have_strict_positive_and_negative_semantics() {
    for path in [
        "fixtures/valid/snapshot-cli.json",
        "fixtures/valid/snapshot-http.json",
        "fixtures/valid/snapshot-metadata.json",
    ] {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../")
                .join(path),
        )
        .expect("fixture");
        parse_snapshot(&bytes).unwrap_or_else(|error| panic!("{path}: {error}"));
    }
    for path in [
        "fixtures/invalid/false-complete.json",
        "fixtures/invalid/duplicate-ordinal.json",
        "fixtures/invalid/duplicate-component-id.json",
        "fixtures/invalid/mapping-missing-component.json",
        "fixtures/invalid/metadata-content-reference.json",
        "fixtures/invalid/missing-output-schema.json",
        "fixtures/invalid/unknown-is-zero.json",
    ] {
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../")
                .join(path),
        )
        .expect("fixture");
        assert!(
            parse_snapshot(&bytes).is_err(),
            "negative fixture accepted: {path}"
        );
    }
}

#[test]
fn metadata_projection_never_reports_content() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/valid/snapshot-metadata.json"
    ))
    .expect("fixture");
    let snapshot = parse_snapshot(&bytes).expect("metadata fixture");
    assert_eq!(snapshot.projection().capture_mode, CaptureMode::Metadata);
    assert!(!snapshot.projection().application_capture_complete);
    assert!(
        snapshot
            .components()
            .iter()
            .all(|component| component.content_status == ComponentStatus::MetadataOnly)
    );
}

#[test]
fn events_are_allowlisted_and_support_plan_reuse_gap() {
    let snapshot = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/valid/snapshot-cli.json"
    ))
    .expect("snapshot");
    let events = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/valid/events.jsonl"
    ))
    .expect("events");
    let mut store = Store::default();
    store.ingest(&snapshot).expect("snapshot ingest");
    let parsed = parse_event_lines(&events).expect("event lines");
    assert_eq!(parsed.len(), 7);
    for line in events
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        store.append_event(line).expect("event ingest");
    }
    assert_eq!(store.event_len(), 7);
    let grant = ReadGrant::issue(
        b"acceptance-token",
        "agent-fixture-001",
        Some("run-fixture-001".to_owned()),
        CapturePrivilege::Content,
        Duration::from_secs(60),
        UNIX_EPOCH,
    )
    .expect("grant");
    let page = store
        .events(
            b"acceptance-token",
            &grant,
            "run-fixture-001",
            None,
            20,
            UNIX_EPOCH,
        )
        .expect("event page");
    assert!(page.events.iter().any(|event| event.sequence == 6));
    assert!(!page.gap);
}

#[test]
fn transport_state_vocabulary_keeps_receipt_unknown_distinct() {
    assert_ne!(
        TransportState::WriteCompleted,
        TransportState::ReceiptReported
    );
    assert_ne!(TransportState::Prepared, TransportState::Unknown);
}
