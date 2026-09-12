// SPDX-License-Identifier: MIT

use serde::Serialize;

use crate::snapshot::Measurement;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum EventType {
    SnapshotPrepared,
    InputWriteCompleted,
    InputWriteFailed,
    ProviderReceiptReported,
    UsageReported,
    DecisionReference,
    ActionReference,
    ContentExpired,
    CaptureGap,
    AttemptInterrupted,
}

impl EventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SnapshotPrepared => "snapshot.prepared",
            Self::InputWriteCompleted => "input.write_completed",
            Self::InputWriteFailed => "input.write_failed",
            Self::ProviderReceiptReported => "provider.receipt_reported",
            Self::UsageReported => "usage.reported",
            Self::DecisionReference => "decision.reference",
            Self::ActionReference => "action.reference",
            Self::ContentExpired => "content.expired",
            Self::CaptureGap => "capture.gap",
            Self::AttemptInterrupted => "attempt.interrupted",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct EventDetails {
    pub code: Option<String>,
    pub component_id: Option<String>,
    pub provider_request_ref: Option<String>,
    pub model_execution_id: Option<String>,
    pub action_id: Option<String>,
    pub plan_id: Option<String>,
    pub usage: Option<Measurement>,
    pub dropped_entries: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CaptureEvent {
    pub event_id: String,
    pub producer_id: String,
    pub sequence: u64,
    pub snapshot_id: Option<String>,
    pub provider_attempt_id: Option<String>,
    pub observed_at: String,
    pub event_type: EventType,
    pub details: EventDetails,
}
