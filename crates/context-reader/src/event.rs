// SPDX-License-Identifier: MIT

//! Bounded parsing for append-only lifecycle evidence.  Event details are deliberately an
//! allowlist: raw provider messages, stderr and reasoning can never enter the read projection.

use serde::Serialize;
use std::fmt;

use super::json::{self, Value};
use super::snapshot::Measurement;

const MAX_EVENT_BYTES: usize = 64 * 1024;
const MAX_EVENT_LINES: usize = 256;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
type Object = [(String, Value)];

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventError {
    Empty,
    TooLarge,
    Json,
    Invalid(&'static str),
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("event is empty"),
            Self::TooLarge => f.write_str("event exceeds its byte bound"),
            Self::Json => f.write_str("invalid event JSON"),
            Self::Invalid(field) => write!(f, "event field is invalid: {field}"),
        }
    }
}

impl std::error::Error for EventError {}

impl From<json::Error> for EventError {
    fn from(_: json::Error) -> Self {
        Self::Json
    }
}

pub fn parse_event(bytes: &[u8]) -> Result<CaptureEvent, EventError> {
    if bytes.is_empty() {
        return Err(EventError::Empty);
    }
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(EventError::TooLarge);
    }
    let parsed = json::parse(bytes)?;
    let root = parsed.object().ok_or(EventError::Invalid("object"))?;
    keys(
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
    if strv(root, "schema")? != "ascension.context-event.v1" {
        return Err(EventError::Invalid("schema"));
    }
    let event_type = event_type(&strv(root, "event_type")?)?;
    let details = details(obj(root, "details")?)?;
    let observed_at = bounded(root, "observed_at", 128)?;
    if !is_rfc3339(&observed_at) {
        return Err(EventError::Invalid("observed_at"));
    }
    let event = CaptureEvent {
        event_id: id_field(root, "event_id")?,
        producer_id: id_field(root, "producer_id")?,
        sequence: number(root, "sequence")?,
        snapshot_id: optional_id(root, "snapshot_id")?,
        provider_attempt_id: optional_id(root, "provider_attempt_id")?,
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

fn validate_details(event_type: EventType, details: &EventDetails) -> Result<(), EventError> {
    let present = |value: &Option<String>| value.is_some();
    match event_type {
        EventType::ProviderReceiptReported if !present(&details.provider_request_ref) => {
            Err(EventError::Invalid("details.provider_request_ref"))
        }
        EventType::UsageReported if details.usage.is_none() => {
            Err(EventError::Invalid("details.usage"))
        }
        EventType::ActionReference
            if !present(&details.action_id) || !present(&details.model_execution_id) =>
        {
            Err(EventError::Invalid("details.action_reference"))
        }
        EventType::ContentExpired if !present(&details.component_id) => {
            Err(EventError::Invalid("details.component_id"))
        }
        EventType::CaptureGap if !present(&details.code) || details.dropped_entries.is_none() => {
            Err(EventError::Invalid("details.capture_gap"))
        }
        EventType::InputWriteFailed if !present(&details.code) => {
            Err(EventError::Invalid("details.code"))
        }
        _ => Ok(()),
    }
}

fn details(o: &Object) -> Result<EventDetails, EventError> {
    if o.len() > 8 {
        return Err(EventError::Invalid("details"));
    }
    let allowed = [
        "code",
        "component_id",
        "provider_request_ref",
        "model_execution_id",
        "action_id",
        "plan_id",
        "usage",
        "dropped_entries",
    ];
    if o.iter().any(|(key, _)| !allowed.contains(&key.as_str())) {
        return Err(EventError::Invalid("details"));
    }
    Ok(EventDetails {
        code: optional_id(o, "code")?,
        component_id: optional_id(o, "component_id")?,
        provider_request_ref: optional_id(o, "provider_request_ref")?,
        model_execution_id: optional_id(o, "model_execution_id")?,
        action_id: optional_id(o, "action_id")?,
        plan_id: optional_id(o, "plan_id")?,
        usage: match field(o, "usage") {
            Some(Value::Object(_)) => Some(measurement(obj(o, "usage")?)?),
            Some(Value::Null) | None => None,
            Some(_) => return Err(EventError::Invalid("details.usage")),
        },
        dropped_entries: match field(o, "dropped_entries") {
            Some(Value::Null) | None => None,
            Some(Value::Number(value)) if *value <= MAX_SAFE_INTEGER => Some(*value),
            Some(Value::Number(_)) => return Err(EventError::Invalid("details.dropped_entries")),
            Some(_) => return Err(EventError::Invalid("details.dropped_entries")),
        },
    })
}

fn measurement(o: &Object) -> Result<Measurement, EventError> {
    keys(
        o,
        &[
            "metric",
            "value",
            "source",
            "scope",
            "measurement_revision",
            "provider_turn_ref",
        ],
    )?;
    let metric = bounded(o, "metric", 64)?;
    if !matches!(
        metric.as_str(),
        "input_tokens"
            | "output_tokens"
            | "cached_input_tokens"
            | "cache_write_input_tokens"
            | "reasoning_output_tokens"
    ) {
        return Err(EventError::Invalid("usage.metric"));
    }
    let source = bounded(o, "source", 64)?;
    if !matches!(
        source.as_str(),
        "unavailable" | "provider_reported" | "local_tokenizer" | "heuristic"
    ) {
        return Err(EventError::Invalid("usage.source"));
    }
    let value = match val(o, "value")? {
        Value::Null => None,
        Value::Number(value) if *value <= MAX_SAFE_INTEGER => Some(*value),
        Value::Number(_) => return Err(EventError::Invalid("usage.value")),
        _ => return Err(EventError::Invalid("usage.value")),
    };
    let measurement_revision = optional_id(o, "measurement_revision")?;
    let provider_turn_ref = optional_id(o, "provider_turn_ref")?;
    if source == "unavailable"
        && (value.is_some() || measurement_revision.is_some() || provider_turn_ref.is_some())
    {
        return Err(EventError::Invalid("usage.unavailable"));
    }
    if source != "unavailable" && value.is_none() {
        return Err(EventError::Invalid("usage.value"));
    }
    if matches!(source.as_str(), "local_tokenizer" | "heuristic") && measurement_revision.is_none()
    {
        return Err(EventError::Invalid("usage.measurement_revision"));
    }
    if source == "provider_reported" && provider_turn_ref.is_none() {
        return Err(EventError::Invalid("usage.provider_turn_ref"));
    }
    let scope = bounded(o, "scope", 64)?;
    if !matches!(
        scope.as_str(),
        "component" | "prepared_input" | "provider_turn" | "episode_cumulative"
    ) {
        return Err(EventError::Invalid("usage.scope"));
    }
    Ok(Measurement {
        metric,
        value,
        source,
        scope,
        measurement_revision,
        provider_turn_ref,
    })
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

fn keys(o: &Object, names: &[&'static str]) -> Result<(), EventError> {
    if names.iter().any(|name| field(o, name).is_none())
        || o.iter().any(|(key, _)| !names.contains(&key.as_str()))
    {
        return Err(EventError::Invalid("unknown or missing field"));
    }
    Ok(())
}

fn field<'a>(o: &'a Object, name: &str) -> Option<&'a Value> {
    o.iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn val<'a>(o: &'a Object, name: &'static str) -> Result<&'a Value, EventError> {
    field(o, name).ok_or(EventError::Invalid(name))
}

fn obj<'a>(o: &'a Object, name: &'static str) -> Result<&'a Object, EventError> {
    val(o, name)?.object().ok_or(EventError::Invalid(name))
}

fn strv(o: &Object, name: &'static str) -> Result<String, EventError> {
    val(o, name)?
        .string()
        .map(str::to_owned)
        .ok_or(EventError::Invalid(name))
}

fn bounded(o: &Object, name: &'static str, max: usize) -> Result<String, EventError> {
    let value = strv(o, name)?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(EventError::Invalid(name));
    }
    Ok(value)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}

fn id_field(o: &Object, name: &'static str) -> Result<String, EventError> {
    let value = bounded(o, name, 128)?;
    if !identifier(&value) {
        return Err(EventError::Invalid(name));
    }
    Ok(value)
}

fn optional_id(o: &Object, name: &'static str) -> Result<Option<String>, EventError> {
    match field(o, name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if identifier(value) => Ok(Some(value.clone())),
        Some(_) => Err(EventError::Invalid(name)),
        None => Ok(None),
    }
}

fn number(o: &Object, name: &'static str) -> Result<u64, EventError> {
    let value = val(o, name)?.number().ok_or(EventError::Invalid(name))?;
    if value > MAX_SAFE_INTEGER {
        return Err(EventError::Invalid(name));
    }
    Ok(value)
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

fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    if ![0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
        .into_iter()
        .all(|range| bytes[range].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    let rest = &bytes[19..];
    let zone = if rest.first() == Some(&b'.') {
        let Some(zone_start) = rest.iter().position(|byte| *byte == b'Z' || *byte == b'z') else {
            return false;
        };
        if zone_start == 1 || !rest[1..zone_start].iter().all(u8::is_ascii_digit) {
            return false;
        }
        &rest[zone_start..]
    } else {
        rest
    };
    if matches!(zone, [b'Z'] | [b'z']) {
        return true;
    }
    zone.len() == 6
        && matches!(zone[0], b'+' | b'-')
        && zone[3] == b':'
        && zone[1..3].iter().all(u8::is_ascii_digit)
        && zone[4..6].iter().all(u8::is_ascii_digit)
}
