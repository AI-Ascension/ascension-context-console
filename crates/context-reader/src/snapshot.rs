// SPDX-License-Identifier: MIT

use serde::Serialize;
use std::fmt;

use super::json::{self, Value};
use crate::MAX_SNAPSHOT_BYTES;

const MAX_COMPONENTS: usize = 128;
const MAX_MAPPINGS: usize = 128;
const MAX_REASONS: usize = 16;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
type Object = [(String, Value)];

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
    raw_len: usize,
    projection: SnapshotProjection,
    components: Vec<Component>,
    mapping: Vec<Mapping>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    Empty,
    TooLarge,
    Json,
    Invalid(&'static str),
}
impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("snapshot is empty"),
            Self::TooLarge => f.write_str("snapshot exceeds its byte bound"),
            Self::Json => f.write_str("invalid JSON"),
            Self::Invalid(field) => write!(f, "snapshot field is invalid: {field}"),
        }
    }
}
impl std::error::Error for SnapshotError {}
impl From<json::Error> for SnapshotError {
    fn from(_: json::Error) -> Self {
        Self::Json
    }
}

impl Snapshot {
    pub fn parse(bytes: &[u8]) -> Result<Self, SnapshotError> {
        if bytes.is_empty() {
            return Err(SnapshotError::Empty);
        }
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(SnapshotError::TooLarge);
        }
        let root = json::parse(bytes)?;
        let root = root.object().ok_or(SnapshotError::Invalid("object"))?;
        keys(
            root,
            &[
                "schema",
                "snapshot_id",
                "identity",
                "producer",
                "boundary",
                "capture_mode",
                "provider",
                "recorded_at",
                "application_capture_complete",
                "incomplete_reasons",
                "parent_snapshot_id",
                "components",
                "mapping",
                "input_measurement",
                "model_context_limit_tokens",
                "model_limit_source",
            ],
        )?;
        if strv(root, "schema")? != "ascension.context-snapshot.v1" {
            return Err(SnapshotError::Invalid("schema"));
        }
        let snapshot_id = id_field(root, "snapshot_id")?;
        let identity = parse_identity(obj(root, "identity")?)?;
        let producer = parse_producer(obj(root, "producer")?)?;
        let boundary = bounded(root, "boundary", 64)?;
        if !matches!(
            boundary.as_str(),
            "harness.request" | "adapter.cli_input" | "adapter.http_body"
        ) {
            return Err(SnapshotError::Invalid("boundary"));
        }
        let capture_mode = capture(&strv(root, "capture_mode")?)?;
        let provider = obj(root, "provider")?;
        keys(provider, &["name", "model", "additional_context"])?;
        let provider_name = id_field(provider, "name")?;
        let provider_model = id_field(provider, "model")?;
        if strv(provider, "additional_context")? != "not_exposed" {
            return Err(SnapshotError::Invalid("provider.additional_context"));
        }
        let recorded_at = bounded(root, "recorded_at", 128)?;
        if !is_rfc3339(&recorded_at) {
            return Err(SnapshotError::Invalid("recorded_at"));
        }
        let complete = val(root, "application_capture_complete")?
            .boolean()
            .ok_or(SnapshotError::Invalid("application_capture_complete"))?;
        let incomplete_reasons = reasons(val(root, "incomplete_reasons")?)?;
        let parent_snapshot_id = optional_id(root, "parent_snapshot_id")?;
        let components = components(val(root, "components")?)?;
        let mapping = mappings(val(root, "mapping")?)?;
        if mapping
            .iter()
            .flat_map(|item| item.component_ids.iter())
            .any(|component_id| {
                !components
                    .iter()
                    .any(|component| component.component_id == *component_id)
            })
        {
            return Err(SnapshotError::Invalid("mapping.component_ids"));
        }
        let input_measurement = measurement(obj(root, "input_measurement")?)?;
        let model_context_limit_tokens = optional_number(root, "model_context_limit_tokens")?;
        let model_limit_source = bounded(root, "model_limit_source", 32)?;
        if !matches!(
            model_limit_source.as_str(),
            "unavailable" | "verified_configuration"
        ) {
            return Err(SnapshotError::Invalid("model_limit_source"));
        }
        if complete
            && (!incomplete_reasons.is_empty()
                || components
                    .iter()
                    .any(|c| c.content_status != ComponentStatus::Complete))
        {
            return Err(SnapshotError::Invalid("application_capture_complete"));
        }
        if !complete && incomplete_reasons.is_empty() {
            return Err(SnapshotError::Invalid("incomplete_reasons"));
        }
        if capture_mode == CaptureMode::Metadata
            && components.iter().any(|component| {
                component.content_ref.is_some()
                    || component.sha256.is_some()
                    || matches!(
                        component.content_status,
                        ComponentStatus::Complete
                            | ComponentStatus::Redacted
                            | ComponentStatus::Partial
                    )
            })
        {
            return Err(SnapshotError::Invalid("metadata_content"));
        }
        if complete && boundary == "adapter.cli_input" {
            let has = |kind: &str| components.iter().any(|component| component.kind == kind);
            if !has("stdin") || !has("output_schema") || !has("configuration") {
                return Err(SnapshotError::Invalid("components.cli_boundary"));
            }
        }
        Ok(Self {
            raw_len: bytes.len(),
            projection: SnapshotProjection {
                snapshot_id,
                identity,
                producer,
                boundary,
                capture_mode,
                provider_name,
                provider_model,
                recorded_at,
                application_capture_complete: complete,
                incomplete_reasons,
                parent_snapshot_id,
                input_measurement,
                model_context_limit_tokens,
                model_limit_source,
                component_count: components.len(),
            },
            components,
            mapping,
        })
    }
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

fn parse_identity(o: &Object) -> Result<Identity, SnapshotError> {
    keys(
        o,
        &[
            "run_id",
            "episode_id",
            "agent_id",
            "model_execution_id",
            "provider_attempt_id",
        ],
    )?;
    Ok(Identity {
        run_id: id_field(o, "run_id")?,
        episode_id: id_field(o, "episode_id")?,
        agent_id: id_field(o, "agent_id")?,
        model_execution_id: id_field(o, "model_execution_id")?,
        provider_attempt_id: id_field(o, "provider_attempt_id")?,
    })
}

fn parse_producer(o: &Object) -> Result<Producer, SnapshotError> {
    keys(
        o,
        &["repository", "revision", "adapter_revision", "evidence"],
    )?;
    let revision = strv(o, "revision")?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(SnapshotError::Invalid("producer.revision"));
    }
    let evidence = strv(o, "evidence")?;
    if !matches!(
        evidence.as_str(),
        "synthetic" | "source-derived" | "native-application-boundary"
    ) {
        return Err(SnapshotError::Invalid("producer.evidence"));
    }
    Ok(Producer {
        repository: bounded(o, "repository", 256)?,
        revision,
        adapter_revision: id_field(o, "adapter_revision")?,
        evidence,
    })
}

fn capture(value: &str) -> Result<CaptureMode, SnapshotError> {
    match value {
        "metadata" => Ok(CaptureMode::Metadata),
        "memory" => Ok(CaptureMode::Memory),
        "private" => Ok(CaptureMode::Private),
        _ => Err(SnapshotError::Invalid("capture_mode")),
    }
}
fn reasons(value: &Value) -> Result<Vec<String>, SnapshotError> {
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

fn components(value: &Value) -> Result<Vec<Component>, SnapshotError> {
    let values = value.array().ok_or(SnapshotError::Invalid("components"))?;
    if values.is_empty() || values.len() > MAX_COMPONENTS {
        return Err(SnapshotError::Invalid("components"));
    }
    let mut result = Vec::with_capacity(values.len());
    for item in values {
        let o = item.object().ok_or(SnapshotError::Invalid("component"))?;
        keys(
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
        let content_status = status(&strv(o, "content_status")?)?;
        let kind = bounded(o, "kind", 64)?;
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
        let origin = bounded(o, "origin", 128)?;
        if !matches!(
            origin.as_str(),
            "harness" | "adapter" | "artifact" | "operator_fixture" | "t02_synthetic_fixture"
        ) {
            return Err(SnapshotError::Invalid("component.origin"));
        }
        let component = Component {
            component_id: id_field(o, "component_id")?,
            ordinal: number(o, "ordinal")?,
            kind,
            role,
            media_type: bounded(o, "media_type", 128)?,
            observed_bytes: number(o, "observed_bytes")?,
            content_status,
            content_ref: optional_id(o, "content_ref")?,
            sha256: digest(o, "sha256")?,
            origin,
            measurement: measurement(obj(o, "measurement")?)?,
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

fn mappings(value: &Value) -> Result<Vec<Mapping>, SnapshotError> {
    let values = value.array().ok_or(SnapshotError::Invalid("mapping"))?;
    if values.len() > MAX_MAPPINGS {
        return Err(SnapshotError::Invalid("mapping"));
    }
    let mut result = Vec::with_capacity(values.len());
    for item in values {
        let o = item.object().ok_or(SnapshotError::Invalid("mapping"))?;
        keys(o, &["upstream_field", "transformation", "component_ids"])?;
        let transformation = strv(o, "transformation")?;
        if !matches!(
            transformation.as_str(),
            "forwarded"
                | "rendered_as_text"
                | "mapped_to_schema"
                | "mapped_to_configuration"
                | "omitted"
                | "unsupported"
        ) {
            return Err(SnapshotError::Invalid("mapping.transformation"));
        }
        let values = val(o, "component_ids")?
            .array()
            .ok_or(SnapshotError::Invalid("mapping.component_ids"))?;
        let mut component_ids = Vec::with_capacity(values.len());
        for value in values {
            let value = value
                .string()
                .ok_or(SnapshotError::Invalid("mapping.component_ids"))?;
            let value =
                identifier(value).map_err(|_| SnapshotError::Invalid("mapping.component_ids"))?;
            if component_ids.iter().any(|old| old == &value) {
                return Err(SnapshotError::Invalid("mapping.component_ids"));
            }
            component_ids.push(value);
        }
        result.push(Mapping {
            upstream_field: id_field(o, "upstream_field")?,
            transformation,
            component_ids,
        });
    }
    Ok(result)
}

fn measurement(o: &Object) -> Result<Measurement, SnapshotError> {
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
        return Err(SnapshotError::Invalid("measurement.metric"));
    }
    let source = bounded(o, "source", 64)?;
    if !matches!(
        source.as_str(),
        "unavailable" | "provider_reported" | "local_tokenizer" | "heuristic"
    ) {
        return Err(SnapshotError::Invalid("measurement.source"));
    }
    let value = val(o, "value")?;
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
    let scope = bounded(o, "scope", 64)?;
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

fn keys(o: &Object, names: &[&'static str]) -> Result<(), SnapshotError> {
    if names.iter().any(|name| field(o, name).is_none())
        || o.iter().any(|(key, _)| !names.contains(&key.as_str()))
    {
        return Err(SnapshotError::Invalid("unknown or missing field"));
    }
    Ok(())
}
fn field<'a>(o: &'a Object, name: &str) -> Option<&'a Value> {
    o.iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}
fn val<'a>(o: &'a Object, name: &'static str) -> Result<&'a Value, SnapshotError> {
    field(o, name).ok_or(SnapshotError::Invalid(name))
}
fn obj<'a>(o: &'a Object, name: &'static str) -> Result<&'a Object, SnapshotError> {
    val(o, name)?.object().ok_or(SnapshotError::Invalid(name))
}
fn strv(o: &Object, name: &'static str) -> Result<String, SnapshotError> {
    val(o, name)?
        .string()
        .map(str::to_owned)
        .ok_or(SnapshotError::Invalid(name))
}
fn bounded(o: &Object, name: &'static str, max: usize) -> Result<String, SnapshotError> {
    let value = strv(o, name)?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(SnapshotError::Invalid(name));
    }
    Ok(value)
}
fn optional_string(
    o: &Object,
    name: &'static str,
    max: usize,
) -> Result<Option<String>, SnapshotError> {
    let value = val(o, name)?;
    if value.is_null() {
        return Ok(None);
    }
    let value = value.string().ok_or(SnapshotError::Invalid(name))?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(SnapshotError::Invalid(name));
    }
    Ok(Some(value.to_owned()))
}
fn identifier(value: &str) -> Result<String, SnapshotError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphanumeric() || (i > 0 && "._:-".contains(c)));
    if valid {
        Ok(value.to_owned())
    } else {
        Err(SnapshotError::Invalid("identifier"))
    }
}
fn id_field(o: &Object, name: &'static str) -> Result<String, SnapshotError> {
    identifier(&strv(o, name)?).map_err(|_| SnapshotError::Invalid(name))
}
fn optional_id(o: &Object, name: &'static str) -> Result<Option<String>, SnapshotError> {
    let value = val(o, name)?;
    if value.is_null() {
        return Ok(None);
    }
    let value = value.string().ok_or(SnapshotError::Invalid(name))?;
    identifier(value)
        .map(Some)
        .map_err(|_| SnapshotError::Invalid(name))
}
fn digest(o: &Object, name: &'static str) -> Result<Option<String>, SnapshotError> {
    let value = val(o, name)?;
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
fn number(o: &Object, name: &'static str) -> Result<u64, SnapshotError> {
    let value = val(o, name)?.number().ok_or(SnapshotError::Invalid(name))?;
    if value > MAX_SAFE_INTEGER {
        return Err(SnapshotError::Invalid(name));
    }
    Ok(value)
}
fn optional_number(o: &Object, name: &'static str) -> Result<Option<u64>, SnapshotError> {
    let value = val(o, name)?;
    if value.is_null() {
        Ok(None)
    } else {
        let value = value.number().ok_or(SnapshotError::Invalid(name))?;
        if value > MAX_SAFE_INTEGER {
            return Err(SnapshotError::Invalid(name));
        }
        Ok(Some(value))
    }
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
