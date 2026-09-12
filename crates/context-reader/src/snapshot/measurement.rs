// SPDX-License-Identifier: MIT

use super::component::optional_string;
use super::error::SnapshotError;
use super::types::Measurement;
use crate::json::{MAX_SAFE_INTEGER, Object, bounded, keys, val};

pub(super) fn measurement(o: &Object) -> Result<Measurement, SnapshotError> {
    keys::<SnapshotError>(
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
    let metric = bounded::<SnapshotError>(o, "metric", 64)?;
    if !matches!(
        metric.as_str(),
        "input_tokens"
            | "output_tokens"
            | "cached_input_tokens"
            | "cache_write_input_tokens"
            | "reasoning_output_tokens"
    ) {
        return Err(SnapshotError::Invalid("measurement.metric"));
    }
    let source = bounded::<SnapshotError>(o, "source", 64)?;
    if !matches!(
        source.as_str(),
        "unavailable" | "provider_reported" | "local_tokenizer" | "heuristic"
    ) {
        return Err(SnapshotError::Invalid("measurement.source"));
    }
    let value = val::<SnapshotError>(o, "value")?;
    let value = if value.is_null() {
        None
    } else {
        Some({
            let value = value
                .number()
                .ok_or(SnapshotError::Invalid("measurement.value"))?;
            if value > MAX_SAFE_INTEGER {
                return Err(SnapshotError::Invalid("measurement.value"));
            }
            value
        })
    };
    let measurement_revision = optional_string(o, "measurement_revision", 128)?;
    let provider_turn_ref = optional_string(o, "provider_turn_ref", 128)?;
    if source == "unavailable"
        && (value.is_some() || measurement_revision.is_some() || provider_turn_ref.is_some())
    {
        return Err(SnapshotError::Invalid("measurement.unavailable"));
    }
    if source != "unavailable" && value.is_none() {
        return Err(SnapshotError::Invalid("measurement.value"));
    }
    if matches!(source.as_str(), "heuristic" | "local_tokenizer") && measurement_revision.is_none()
    {
        return Err(SnapshotError::Invalid("measurement.measurement_revision"));
    }
    if source == "provider_reported" && provider_turn_ref.is_none() {
        return Err(SnapshotError::Invalid("measurement.provider_turn_ref"));
    }
    let scope = bounded::<SnapshotError>(o, "scope", 64)?;
    if !matches!(
        scope.as_str(),
        "component" | "prepared_input" | "provider_turn" | "episode_cumulative"
    ) {
        return Err(SnapshotError::Invalid("measurement.scope"));
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
