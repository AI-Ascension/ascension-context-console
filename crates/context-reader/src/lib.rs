// SPDX-License-Identifier: MIT

//! Bounded, read-only validation for Context Console snapshot manifests.
//!
//! The crate deliberately uses only the Rust standard library.  The target manifest and
//! dependency wiring are root-owned; this source can therefore be reviewed and tested before
//! those shared files are added.

mod association;
mod event;
mod json;
mod snapshot;

pub use association::{
    AssociationError, CaptureEvidence as AssociationCaptureEvidence,
    ContextBinding as AssociationContextBinding,
    InspectionCapabilities as AssociationInspectionCapabilities, WorkflowContextAssociation,
    WorkflowIdentity as AssociationWorkflowIdentity,
};

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

/// Parse a bounded, redacted association emitted by the workflow owner.
pub fn parse_workflow_context_association(
    bytes: &[u8],
) -> Result<WorkflowContextAssociation, AssociationError> {
    WorkflowContextAssociation::parse(bytes)
}
