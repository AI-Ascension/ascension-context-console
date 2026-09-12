// SPDX-License-Identifier: MIT

use context_reader::parse_snapshot;

#[test]
fn synthetic_fixture_is_a_metadata_only_snapshot() {
    let bytes = include_bytes!("../../../fixtures/synthetic/snapshot.json");
    let result = parse_snapshot(bytes);
    assert!(result.is_ok());
    if let Ok(snapshot) = result {
        assert_eq!(snapshot.projection().boundary, "adapter.http_body");
        assert!(!snapshot.projection().application_capture_complete);
        assert_eq!(snapshot.components().len(), 1);
    }
}
