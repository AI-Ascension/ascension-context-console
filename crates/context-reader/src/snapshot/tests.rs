// SPDX-License-Identifier: MIT

use super::*;
use crate::MAX_SNAPSHOT_BYTES;

const FIXTURE: &[u8] = include_bytes!("../../../../fixtures/synthetic/snapshot.json");

#[test]
fn parses_bounded_synthetic_projection() {
    let result = Snapshot::parse(FIXTURE);
    assert!(result.is_ok());
    if let Ok(snapshot) = result {
        assert_eq!(snapshot.projection().capture_mode, CaptureMode::Metadata);
        assert_eq!(snapshot.projection().component_count, 1);
        assert_eq!(
            snapshot.components()[0].content_status,
            ComponentStatus::MetadataOnly
        );
        assert_eq!(snapshot.mapping()[1].transformation, "omitted");
    }
}

#[test]
fn complete_claim_requires_complete_components() {
    let source = String::from_utf8_lossy(FIXTURE);
    let changed = source.replace(
        "\"application_capture_complete\": false",
        "\"application_capture_complete\": true",
    );
    assert!(Snapshot::parse(changed.as_bytes()).is_err());
}

#[test]
fn duplicate_keys_and_oversized_input_fail_closed() {
    assert!(Snapshot::parse(br#"{"schema":"a","schema":"b"}"#).is_err());
    let oversized = vec![b' '; MAX_SNAPSHOT_BYTES + 1];
    assert!(Snapshot::parse(&oversized).is_err());
}
