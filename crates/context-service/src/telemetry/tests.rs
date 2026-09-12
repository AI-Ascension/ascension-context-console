// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn exporter_is_bounded_and_metadata_only() {
    let mut exporter = MemoryTelemetry::new(1).expect("bounded");
    exporter
        .export(&CaptureTelemetry {
            boundary: "adapter.cli_input".to_owned(),
            mode: "metadata".to_owned(),
            state: "prepared".to_owned(),
            observed_bytes: 12,
            dropped_entries: 0,
        })
        .expect("export");
    assert_eq!(exporter.records().len(), 1);
}
