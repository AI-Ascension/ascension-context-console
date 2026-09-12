// SPDX-License-Identifier: MIT

use super::details::{details, validate_details};
use super::error::EventError;
use super::types::{CaptureEvent, EventType};
use crate::json::{self, bounded, id_field, is_rfc3339, keys, number, obj, optional_id, strv};

const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_LINES: usize = 256;

pub fn parse_event(bytes: &[u8]) -> Result<CaptureEvent, EventError> {
    if bytes.is_empty() {
        return Err(EventError::Empty);
    }
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(EventError::TooLarge);
    }
    let parsed = json::parse(bytes)?;
    let root = parsed.object().ok_or(EventError::Invalid("object"))?;
    keys::<EventError>(
        root,
        &[
            "schema",
            "event_id",
            "producer_id",
            "sequence",
            "snapshot_id",
            "provider_attempt_id",
            "observed_at",
            "event_type",
            "details",
        ],
    )?;
    if strv::<EventError>(root, "schema")? != "ascension.context-event.v1" {
        return Err(EventError::Invalid("schema"));
    }
    let event_type = event_type(&strv::<EventError>(root, "event_type")?)?;
    let details = details(obj::<EventError>(root, "details")?)?;
    let observed_at = bounded::<EventError>(root, "observed_at", 128)?;
    if !is_rfc3339(&observed_at) {
        return Err(EventError::Invalid("observed_at"));
    }
    let event = CaptureEvent {
        event_id: id_field::<EventError>(root, "event_id")?,
        producer_id: id_field::<EventError>(root, "producer_id")?,
        sequence: number::<EventError>(root, "sequence")?,
        snapshot_id: optional_id::<EventError>(root, "snapshot_id")?,
        provider_attempt_id: optional_id::<EventError>(root, "provider_attempt_id")?,
        observed_at,
        event_type,
        details,
    };
    validate_details(event.event_type, &event.details)?;
    Ok(event)
}

pub fn parse_event_lines(bytes: &[u8]) -> Result<Vec<CaptureEvent>, EventError> {
    if bytes.len() > MAX_EVENT_BYTES.saturating_mul(MAX_EVENT_LINES) {
        return Err(EventError::TooLarge);
    }
    let mut events = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = trim_ascii_whitespace(line);
        if line.is_empty() {
            continue;
        }
        if events.len() >= MAX_EVENT_LINES {
            return Err(EventError::TooLarge);
        }
        events.push(parse_event(line)?);
    }
    Ok(events)
}

fn event_type(value: &str) -> Result<EventType, EventError> {
    match value {
        "snapshot.prepared" => Ok(EventType::SnapshotPrepared),
        "input.write_completed" => Ok(EventType::InputWriteCompleted),
        "input.write_failed" => Ok(EventType::InputWriteFailed),
        "provider.receipt_reported" => Ok(EventType::ProviderReceiptReported),
        "usage.reported" => Ok(EventType::UsageReported),
        "decision.reference" => Ok(EventType::DecisionReference),
        "action.reference" => Ok(EventType::ActionReference),
        "content.expired" => Ok(EventType::ContentExpired),
        "capture.gap" => Ok(EventType::CaptureGap),
        "attempt.interrupted" => Ok(EventType::AttemptInterrupted),
        _ => Err(EventError::Invalid("event_type")),
    }
}

fn trim_ascii_whitespace(input: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = input.len();
    while start < end && input[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && input[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &input[start..end]
}
