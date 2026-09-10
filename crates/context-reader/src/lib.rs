// SPDX-License-Identifier: MIT

//! Bounded, read-only validation for Context Console snapshot manifests.
//!
//! The crate deliberately uses only the Rust standard library.  The target manifest and
//! dependency wiring are root-owned; this source can therefore be reviewed and tested before
//! those shared files are added.

mod event;
mod json;
mod snapshot;

pub use event::{
    CaptureEvent, EventDetails, EventError, EventType, parse_event, parse_event_lines,
};
pub use snapshot::{
    CaptureMode, Component, ComponentStatus, Identity, Mapping, Measurement, Producer, Snapshot,
    SnapshotError, SnapshotProjection,
};

/// Maximum accepted encoded snapshot size.
pub const MAX_SNAPSHOT_BYTES: usize = 1_048_576;

/// Parse one immutable snapshot manifest without retaining a mutable JSON tree.
pub fn parse_snapshot(bytes: &[u8]) -> Result<Snapshot, SnapshotError> {
    Snapshot::parse(bytes)
}

#[cfg(test)]
mod tests {
    use super::{CaptureMode, ComponentStatus, MAX_SNAPSHOT_BYTES, Snapshot};

    const FIXTURE: &[u8] = include_bytes!("../../../fixtures/synthetic/snapshot.json");

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
}
