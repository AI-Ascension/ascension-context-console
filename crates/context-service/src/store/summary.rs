// SPDX-License-Identifier: MIT

use context_reader::{CaptureEvent, Snapshot};
use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotSummary {
    pub snapshot_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub boundary: String,
    pub capture_mode: String,
    pub component_count: usize,
    pub application_capture_complete: bool,
    pub incomplete_reasons: Vec<String>,
}

impl SnapshotSummary {
    pub(super) fn from(snapshot: &Snapshot) -> Self {
        let projection = snapshot.projection();
        Self {
            snapshot_id: projection.snapshot_id.clone(),
            run_id: projection.identity.run_id.clone(),
            episode_id: projection.identity.episode_id.clone(),
            boundary: projection.boundary.clone(),
            capture_mode: projection.capture_mode.as_str().to_owned(),
            component_count: projection.component_count,
            application_capture_complete: projection.application_capture_complete,
            incomplete_reasons: projection.incomplete_reasons.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ComponentSummary {
    pub snapshot_id: String,
    pub component_id: String,
    pub ordinal: u64,
    pub kind: String,
    pub role: Option<String>,
    pub media_type: String,
    pub observed_bytes: u64,
    pub content_status: String,
    pub content_available: bool,
    pub digest_present: bool,
    pub measurement: context_reader::Measurement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventPage {
    pub events: Vec<CaptureEvent>,
    /// An authenticated reconnect cursor bound to the bearer grant and run stream.
    pub next_cursor: Option<String>,
    pub gap: bool,
}
