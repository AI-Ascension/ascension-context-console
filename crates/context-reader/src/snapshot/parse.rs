// SPDX-License-Identifier: MIT

use super::component::{components, reasons};
use super::error::SnapshotError;
use super::mapping::mappings;
use super::measurement::measurement;
use super::types::{
    CaptureMode, ComponentStatus, Identity, Producer, Snapshot, SnapshotProjection,
};
use crate::MAX_SNAPSHOT_BYTES;
use crate::json::{
    self, Object, bounded, id_field, is_rfc3339, keys, obj, optional_id, optional_number, strv, val,
};

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
        keys::<SnapshotError>(
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
        if strv::<SnapshotError>(root, "schema")? != "ascension.context-snapshot.v1" {
            return Err(SnapshotError::Invalid("schema"));
        }
        let snapshot_id = id_field::<SnapshotError>(root, "snapshot_id")?;
        let identity = parse_identity(obj::<SnapshotError>(root, "identity")?)?;
        let producer = parse_producer(obj::<SnapshotError>(root, "producer")?)?;
        let boundary = bounded::<SnapshotError>(root, "boundary", 64)?;
        if !matches!(
            boundary.as_str(),
            "harness.request" | "adapter.cli_input" | "adapter.http_body"
        ) {
            return Err(SnapshotError::Invalid("boundary"));
        }
        let capture_mode = capture(&strv::<SnapshotError>(root, "capture_mode")?)?;
        let provider = obj::<SnapshotError>(root, "provider")?;
        keys::<SnapshotError>(provider, &["name", "model", "additional_context"])?;
        let provider_name = id_field::<SnapshotError>(provider, "name")?;
        let provider_model = id_field::<SnapshotError>(provider, "model")?;
        if strv::<SnapshotError>(provider, "additional_context")? != "not_exposed" {
            return Err(SnapshotError::Invalid("provider.additional_context"));
        }
        let recorded_at = bounded::<SnapshotError>(root, "recorded_at", 128)?;
        if !is_rfc3339(&recorded_at) {
            return Err(SnapshotError::Invalid("recorded_at"));
        }
        let complete = val::<SnapshotError>(root, "application_capture_complete")?
            .boolean()
            .ok_or(SnapshotError::Invalid("application_capture_complete"))?;
        let incomplete_reasons = reasons(val::<SnapshotError>(root, "incomplete_reasons")?)?;
        let parent_snapshot_id = optional_id::<SnapshotError>(root, "parent_snapshot_id")?;
        let components = components(val::<SnapshotError>(root, "components")?)?;
        let mapping = mappings(val::<SnapshotError>(root, "mapping")?)?;
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
        let input_measurement = measurement(obj::<SnapshotError>(root, "input_measurement")?)?;
        let model_context_limit_tokens =
            optional_number::<SnapshotError>(root, "model_context_limit_tokens")?;
        let model_limit_source = bounded::<SnapshotError>(root, "model_limit_source", 32)?;
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
}

fn parse_identity(o: &Object) -> Result<Identity, SnapshotError> {
    keys::<SnapshotError>(
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
        run_id: id_field::<SnapshotError>(o, "run_id")?,
        episode_id: id_field::<SnapshotError>(o, "episode_id")?,
        agent_id: id_field::<SnapshotError>(o, "agent_id")?,
        model_execution_id: id_field::<SnapshotError>(o, "model_execution_id")?,
        provider_attempt_id: id_field::<SnapshotError>(o, "provider_attempt_id")?,
    })
}

fn parse_producer(o: &Object) -> Result<Producer, SnapshotError> {
    keys::<SnapshotError>(
        o,
        &["repository", "revision", "adapter_revision", "evidence"],
    )?;
    let revision = strv::<SnapshotError>(o, "revision")?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(SnapshotError::Invalid("producer.revision"));
    }
    let evidence = strv::<SnapshotError>(o, "evidence")?;
    if !matches!(
        evidence.as_str(),
        "synthetic" | "source-derived" | "native-application-boundary"
    ) {
        return Err(SnapshotError::Invalid("producer.evidence"));
    }
    Ok(Producer {
        repository: bounded::<SnapshotError>(o, "repository", 256)?,
        revision,
        adapter_revision: id_field::<SnapshotError>(o, "adapter_revision")?,
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
