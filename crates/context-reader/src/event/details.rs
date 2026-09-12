// SPDX-License-Identifier: MIT

use super::error::EventError;
use super::types::{EventDetails, EventType};
use crate::json::{MAX_SAFE_INTEGER, Object, Value, bounded, field, keys, obj, optional_id, val};
use crate::snapshot::Measurement;

pub(super) fn details(o: &Object) -> Result<EventDetails, EventError> {
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
        code: optional_id::<EventError>(o, "code")?,
        component_id: optional_id::<EventError>(o, "component_id")?,
        provider_request_ref: optional_id::<EventError>(o, "provider_request_ref")?,
        model_execution_id: optional_id::<EventError>(o, "model_execution_id")?,
        action_id: optional_id::<EventError>(o, "action_id")?,
        plan_id: optional_id::<EventError>(o, "plan_id")?,
        usage: match field(o, "usage") {
            Some(Value::Object(_)) => Some(measurement(obj::<EventError>(o, "usage")?)?),
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
    keys::<EventError>(
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
    let metric = bounded::<EventError>(o, "metric", 64)?;
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
    let source = bounded::<EventError>(o, "source", 64)?;
    if !matches!(
        source.as_str(),
        "unavailable" | "provider_reported" | "local_tokenizer" | "heuristic"
    ) {
        return Err(EventError::Invalid("usage.source"));
    }
    let value = match val::<EventError>(o, "value")? {
        Value::Null => None,
        Value::Number(value) if *value <= MAX_SAFE_INTEGER => Some(*value),
        Value::Number(_) => return Err(EventError::Invalid("usage.value")),
        _ => return Err(EventError::Invalid("usage.value")),
    };
    let measurement_revision = optional_id::<EventError>(o, "measurement_revision")?;
    let provider_turn_ref = optional_id::<EventError>(o, "provider_turn_ref")?;
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
    let scope = bounded::<EventError>(o, "scope", 64)?;
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

pub(super) fn validate_details(
    event_type: EventType,
    details: &EventDetails,
) -> Result<(), EventError> {
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
