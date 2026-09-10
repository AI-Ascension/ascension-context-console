// SPDX-License-Identifier: MIT

//! Pure preparation for the enabled management profile.
//!
//! The returned bytes are the same bytes a bridge would submit.  Preview code calls this
//! function, and the explicit resume path retains the resulting material rather than rendering a
//! second approximation.

use super::types::{Boundary, Draft, ItemRecord, ItemRef, MAX_COMPONENT_BYTES, PreviewComponent};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const OUTPUT_SCHEMA: &[u8] = br#"{"type":"object","properties":{"action_ids":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"string"}},"rationale":{"type":"string","maxLength":512}},"required":["action_ids","rationale"],"additionalProperties":false}"#;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PreparedMaterial {
    pub input: Vec<u8>,
    pub schema: Vec<u8>,
    pub configuration: Vec<u8>,
    pub components: Vec<PreviewComponent>,
    pub manifest_sha256: String,
}

pub(crate) fn output_schema() -> &'static [u8] {
    OUTPUT_SCHEMA
}

pub(crate) fn render(
    preview_id: &str,
    boundary: &Boundary,
    draft: &Draft,
    registry: &BTreeMap<(String, u64), ItemRecord>,
    now: u64,
) -> Result<PreparedMaterial, String> {
    let mut selected = Vec::with_capacity(draft.selected_items.len());
    for item in &draft.selected_items {
        let record = registry
            .get(&(item.item_id.clone(), item.version))
            .ok_or_else(|| format!("unknown item {}", item.item_id))?;
        if record.item != *item {
            return Err(format!("item digest mismatch for {}", item.item_id));
        }
        if record.protected {
            return Err(format!(
                "protected item {} cannot be rendered as an edit",
                item.item_id
            ));
        }
        if record.expires_at <= now || record.content.is_empty() {
            return Err(format!("item {} is expired or unavailable", item.item_id));
        }
        let content = std::str::from_utf8(&record.content)
            .map_err(|_| format!("item {} is not UTF-8 text", item.item_id))?;
        selected.push(json!({
            "item_id": item.item_id,
            "version": item.version,
            "sha256": item.sha256,
            "kind": record.kind,
            "content": content,
        }));
    }

    let objective = draft
        .objective_item
        .as_ref()
        .map(|item| {
            let record = registry
                .get(&(item.item_id.clone(), item.version))
                .ok_or_else(|| "objective content is unavailable".to_owned())?;
            if record.item != *item || record.protected || record.expires_at <= now {
                return Err("objective content is unavailable".to_owned());
            }
            std::str::from_utf8(&record.content)
                .map(str::to_owned)
                .map_err(|_| "objective content is unavailable".to_owned())
        })
        .transpose()?;
    let notes = draft
        .note_items
        .iter()
        .map(|item| {
            let record = registry
                .get(&(item.item_id.clone(), item.version))
                .ok_or_else(|| "note content is unavailable".to_owned())?;
            if record.item != *item || record.protected || record.expires_at <= now {
                return Err("note content is unavailable".to_owned());
            }
            let content = std::str::from_utf8(&record.content)
                .map_err(|_| "note content is unavailable".to_owned())?;
            Ok(json!({
                "item_id": item.item_id,
                "version": item.version,
                "sha256": item.sha256,
                "attributed_to": draft.author_ref,
                "content": content,
            }))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let protected = json!({
        "state_id": boundary.state_id,
        "generation": boundary.generation,
        "observation_sha256": boundary.observation_sha256,
        "catalog_sha256": boundary.catalog_sha256,
        "legal_actions": ["combat.end-turn"],
        "authority": "harness-owned",
    });
    let input_value = json!({
        "schema": "ascension.context-control.prepared-input.v1",
        "profile": "management-enabled",
        "scope": boundary.scope,
        "protected_state": protected,
        "objective": objective,
        "notes": notes,
        "selected_items": selected,
        "constraints": ["Use only visible state", "Return only legal action IDs"],
    });
    let input = serde_json::to_vec(&input_value).map_err(|_| "input encoding failed".to_owned())?;
    let configuration = serde_json::to_vec(&json!({
        "profile": "management-enabled/v1",
        "adapter_revision": boundary.adapter_revision,
        "model": boundary.model,
        "provider_added_context": "not_exposed",
        "tools": [],
        "credentials": "excluded",
    }))
    .map_err(|_| "configuration encoding failed".to_owned())?;
    if input.len() > MAX_COMPONENT_BYTES {
        return Err("mandatory_budget_exceeded".to_owned());
    }

    let components = vec![
        component(
            &format!("input-{preview_id}"),
            0,
            "stdin",
            &input,
            false,
            &format!("prepared-input-{preview_id}"),
        ),
        component(
            &format!("schema-{preview_id}"),
            1,
            "output_schema",
            OUTPUT_SCHEMA,
            true,
            &format!("output-schema-{preview_id}"),
        ),
        component(
            &format!("configuration-{preview_id}"),
            2,
            "configuration",
            &configuration,
            true,
            &format!("configuration-{preview_id}"),
        ),
    ];
    let manifest = json!({
        "manifest_version": "ascension.context-control.prepared-material.v1",
        "adapter_revision": boundary.adapter_revision,
        "model": boundary.model,
        "configuration_sha256": boundary.configuration_sha256,
        "output_schema_sha256": boundary.output_schema_sha256,
        "components": components,
    });
    let manifest_bytes =
        serde_json::to_vec(&manifest).map_err(|_| "manifest encoding failed".to_owned())?;
    Ok(PreparedMaterial {
        input,
        schema: OUTPUT_SCHEMA.to_vec(),
        configuration,
        components,
        manifest_sha256: digest(&manifest_bytes),
    })
}

fn component(
    component_id: &str,
    ordinal: u64,
    kind: &str,
    bytes: &[u8],
    protected: bool,
    content_ref: &str,
) -> PreviewComponent {
    PreviewComponent {
        component_id: component_id.to_owned(),
        ordinal,
        kind: kind.to_owned(),
        sha256: digest(bytes),
        bytes: bytes.len(),
        protected,
        content_ref: content_ref.to_owned(),
    }
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn item_ref(item_id: impl Into<String>, version: u64, content: &[u8]) -> ItemRef {
    ItemRef {
        item_id: item_id.into(),
        version,
        sha256: digest(content),
    }
}
