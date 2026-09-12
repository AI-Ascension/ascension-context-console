// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn disabled_path_does_not_hash_or_copy() {
    let mut sink = NoopCapture;
    let bytes = b"synthetic-capture-canary";
    assert!(
        sink.prepared(PreparedCapture {
            snapshot_id: "snapshot-1",
            attempt_id: "attempt-1",
            boundary: "adapter.cli_input",
            bytes,
        })
        .is_ok()
    );
}

#[test]
fn metadata_omits_digest_and_memory_bounds_queue() {
    let mut sink = MemoryCapture::new(CaptureConfig {
        mode: CaptureMode::Metadata,
        max_queue_entries: 1,
        max_record_bytes: 128,
    })
    .expect("valid config");
    for id in ["snapshot-1", "snapshot-2"] {
        sink.prepared(PreparedCapture {
            snapshot_id: id,
            attempt_id: "attempt-1",
            boundary: "adapter.cli_input",
            bytes: b"input",
        })
        .expect("capture");
    }
    assert_eq!(sink.records().count(), 1);
    assert_eq!(sink.dropped_entries(), 1);
    assert!(sink.records().next().expect("record").digest.is_none());
}

#[test]
fn private_mode_is_rejected_until_an_approved_vault_is_wired() {
    assert!(matches!(
        MemoryCapture::new(CaptureConfig {
            mode: CaptureMode::Private,
            max_queue_entries: 1,
            max_record_bytes: 128,
        }),
        Err(CaptureError::PrivateRequiresVault)
    ));
}
