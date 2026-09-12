// SPDX-License-Identifier: MIT

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    Metadata,
    Memory,
    Private,
}
impl CaptureMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Memory => "memory",
            Self::Private => "private",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentStatus {
    Complete,
    MetadataOnly,
    Redacted,
    Partial,
    Unavailable,
    Expired,
}
impl ComponentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::MetadataOnly => "metadata_only",
            Self::Redacted => "redacted",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
            Self::Expired => "expired",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity {
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
    pub model_execution_id: String,
    pub provider_attempt_id: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Producer {
    pub repository: String,
    pub revision: String,
    pub adapter_revision: String,
    pub evidence: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Measurement {
    pub metric: String,
    pub value: Option<u64>,
    pub source: String,
    pub scope: String,
    pub measurement_revision: Option<String>,
    pub provider_turn_ref: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Component {
    pub component_id: String,
    pub ordinal: u64,
    pub kind: String,
    pub role: Option<String>,
    pub media_type: String,
    pub observed_bytes: u64,
    pub content_status: ComponentStatus,
    pub content_ref: Option<String>,
    pub sha256: Option<String>,
    pub origin: String,
    pub measurement: Measurement,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mapping {
    pub upstream_field: String,
    pub transformation: String,
    pub component_ids: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotProjection {
    pub snapshot_id: String,
    pub identity: Identity,
    pub producer: Producer,
    pub boundary: String,
    pub capture_mode: CaptureMode,
    pub provider_name: String,
    pub provider_model: String,
    pub recorded_at: String,
    pub application_capture_complete: bool,
    pub incomplete_reasons: Vec<String>,
    pub parent_snapshot_id: Option<String>,
    pub input_measurement: Measurement,
    pub model_context_limit_tokens: Option<u64>,
    pub model_limit_source: String,
    pub component_count: usize,
}

/// Immutable, bounded projection; raw JSON and private component bytes are not retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub(super) raw_len: usize,
    pub(super) projection: SnapshotProjection,
    pub(super) components: Vec<Component>,
    pub(super) mapping: Vec<Mapping>,
}

impl Snapshot {
    pub fn raw_len(&self) -> usize {
        self.raw_len
    }
    pub fn projection(&self) -> &SnapshotProjection {
        &self.projection
    }
    pub fn components(&self) -> &[Component] {
        &self.components
    }
    pub fn mapping(&self) -> &[Mapping] {
        &self.mapping
    }
}
