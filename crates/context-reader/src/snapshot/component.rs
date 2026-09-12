// SPDX-License-Identifier: MIT

use super::error::SnapshotError;
use super::measurement::measurement;
use super::types::{Component, ComponentStatus};
use crate::json::{Object, Value, bounded, id_field, keys, number, obj, optional_id, strv};

const MAX_COMPONENTS: usize = 128;
const MAX_REASONS: usize = 16;

pub(super) fn reasons(value: &Value) -> Result<Vec<String>, SnapshotError> {
    let values = value
        .array()
        .ok_or(SnapshotError::Invalid("incomplete_reasons"))?;
    if values.len() > MAX_REASONS {
        return Err(SnapshotError::Invalid("incomplete_reasons"));
    }
    let mut result = Vec::with_capacity(values.len());
    for item in values {
        let reason = item
            .string()
            .ok_or(SnapshotError::Invalid("incomplete_reasons"))?;
        if !matches!(
            reason,
            "content_excluded"
                | "component_missing"
                | "unsupported_attachment"
                | "capture_limit"
                | "redacted"
                | "partial_write_capture"
                | "producer_unknown"
                | "metadata_only"
        ) || result.iter().any(|seen| seen == reason)
        {
            return Err(SnapshotError::Invalid("incomplete_reasons"));
        }
        result.push(reason.to_owned());
    }
    Ok(result)
}

pub(super) fn components(value: &Value) -> Result<Vec<Component>, SnapshotError> {
    let values = value.array().ok_or(SnapshotError::Invalid("components"))?;
    if values.is_empty() || values.len() > MAX_COMPONENTS {
        return Err(SnapshotError::Invalid("components"));
    }
    let mut result = Vec::with_capacity(values.len());
    for item in values {
        let o = item.object().ok_or(SnapshotError::Invalid("component"))?;
        keys::<SnapshotError>(
            o,
            &[
                "component_id",
                "ordinal",
                "kind",
                "role",
                "media_type",
                "observed_bytes",
                "content_status",
                "content_ref",
                "sha256",
                "origin",
                "measurement",
            ],
        )?;
        let content_status = status(&strv::<SnapshotError>(o, "content_status")?)?;
        let kind = bounded::<SnapshotError>(o, "kind", 64)?;
        if !matches!(
            kind.as_str(),
            "harness_request"
                | "stdin"
                | "serialized_http_body"
                | "system_message"
                | "user_message"
                | "output_schema"
                | "configuration"
                | "attachment"
                | "opaque"
        ) {
            return Err(SnapshotError::Invalid("component.kind"));
        }
        let role = optional_string(o, "role", 128)?;
        if role.as_deref().is_some_and(|value| {
            !matches!(
                value,
                "system" | "developer" | "user" | "assistant" | "tool"
            )
        }) {
            return Err(SnapshotError::Invalid("component.role"));
        }
        let origin = bounded::<SnapshotError>(o, "origin", 128)?;
        if !matches!(
            origin.as_str(),
            "harness" | "adapter" | "artifact" | "operator_fixture" | "t02_synthetic_fixture"
        ) {
            return Err(SnapshotError::Invalid("component.origin"));
        }
        let component = Component {
            component_id: id_field::<SnapshotError>(o, "component_id")?,
            ordinal: number::<SnapshotError>(o, "ordinal")?,
            kind,
            role,
            media_type: bounded::<SnapshotError>(o, "media_type", 128)?,
            observed_bytes: number::<SnapshotError>(o, "observed_bytes")?,
            content_status,
            content_ref: optional_id::<SnapshotError>(o, "content_ref")?,
            sha256: digest(o, "sha256")?,
            origin,
            measurement: measurement(obj::<SnapshotError>(o, "measurement")?)?,
        };
        let content_present = component.content_ref.is_some() || component.sha256.is_some();
        let content_required = matches!(
            component.content_status,
            ComponentStatus::Complete | ComponentStatus::Redacted | ComponentStatus::Partial
        );
        let content_forbidden = matches!(
            component.content_status,
            ComponentStatus::MetadataOnly | ComponentStatus::Unavailable | ComponentStatus::Expired
        );
        if (content_required
            && (!content_present || component.content_ref.is_none() || component.sha256.is_none()))
            || (content_forbidden && content_present)
            || result.iter().any(|old: &Component| {
                old.component_id == component.component_id || old.ordinal == component.ordinal
            })
        {
            return Err(SnapshotError::Invalid("components"));
        }
        result.push(component);
    }
    if result
        .iter()
        .enumerate()
        .any(|(index, component)| component.ordinal != index as u64)
    {
        return Err(SnapshotError::Invalid("components.ordinal"));
    }
    Ok(result)
}

fn status(value: &str) -> Result<ComponentStatus, SnapshotError> {
    match value {
        "complete" => Ok(ComponentStatus::Complete),
        "metadata_only" => Ok(ComponentStatus::MetadataOnly),
        "redacted" => Ok(ComponentStatus::Redacted),
        "partial" => Ok(ComponentStatus::Partial),
        "unavailable" => Ok(ComponentStatus::Unavailable),
        "expired" => Ok(ComponentStatus::Expired),
        _ => Err(SnapshotError::Invalid("component.content_status")),
    }
}

pub(super) fn optional_string(
    o: &Object,
    name: &'static str,
    max: usize,
) -> Result<Option<String>, SnapshotError> {
    let value = crate::json::val::<SnapshotError>(o, name)?;
    if value.is_null() {
        return Ok(None);
    }
    let value = value.string().ok_or(SnapshotError::Invalid(name))?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(SnapshotError::Invalid(name));
    }
    Ok(Some(value.to_owned()))
}

fn digest(o: &Object, name: &'static str) -> Result<Option<String>, SnapshotError> {
    let value = crate::json::val::<SnapshotError>(o, name)?;
    if value.is_null() {
        return Ok(None);
    }
    let value = value.string().ok_or(SnapshotError::Invalid(name))?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(SnapshotError::Invalid(name));
    }
    Ok(Some(value.to_owned()))
}
