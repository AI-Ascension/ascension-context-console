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

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::{item_ref, output_schema, render};
    use crate::control::state::ControlPlane;
    use crate::control::types::{Draft, ItemRecord, ItemRef, MAX_ITEMS, Scope};
    use serde_json::Value;
    use std::collections::BTreeMap;

    fn draft(scope: Scope, selected_items: Vec<ItemRef>) -> Draft {
        Draft {
            schema: "ascension.context-control.draft.v1".to_owned(),
            scope,
            draft_id: "draft-render-contract".to_owned(),
            version: 1,
            base_revision_id: "revision-1".to_owned(),
            selected_items,
            pinned_item_ids: Vec::new(),
            note_items: Vec::new(),
            objective_item: None,
            author_ref: "operator-render".to_owned(),
            expires_at: "2100-01-01T00:00:00Z".to_owned(),
        }
    }

    fn record(
        item: ItemRef,
        scope: Scope,
        kind: &str,
        protected: bool,
        content: Vec<u8>,
        expires_at: u64,
    ) -> ItemRecord {
        ItemRecord {
            item,
            kind: kind.to_owned(),
            protected,
            scope,
            content,
            expires_at,
            expires_text: if expires_at == 4_102_444_800 {
                "2100-01-01T00:00:00Z".to_owned()
            } else {
                "2000-01-01T00:00:00Z".to_owned()
            },
            locked_reason: protected.then(|| "host-owned".to_owned()),
        }
    }

    #[test]
    fn mandatory_context_over_adapter_budget_is_rejected_without_trimming() {
        let plane = ControlPlane::synthetic();
        let boundary = plane.state().boundary.expect("synthetic boundary");
        let scope: Scope = boundary.scope.clone();
        let mut registry = BTreeMap::new();
        let mut selected_items: Vec<ItemRef> = Vec::with_capacity(MAX_ITEMS);
        for index in 0..MAX_ITEMS {
            let item_id = format!("large-item-{index}");
            let content = vec![b'x'; 3_000];
            let item = item_ref(item_id.clone(), 1, &content);
            registry.insert(
                (item_id, 1),
                ItemRecord {
                    item: item.clone(),
                    kind: "history".to_owned(),
                    protected: false,
                    scope: scope.clone(),
                    content,
                    expires_at: 4_102_444_800,
                    expires_text: "2100-01-01T00:00:00Z".to_owned(),
                    locked_reason: None,
                },
            );
            selected_items.push(item);
        }
        let draft = Draft {
            schema: "ascension.context-control.draft.v1".to_owned(),
            scope,
            draft_id: "draft-budget".to_owned(),
            version: 1,
            base_revision_id: "revision-1".to_owned(),
            selected_items,
            pinned_item_ids: Vec::new(),
            note_items: Vec::new(),
            objective_item: None,
            author_ref: "operator-budget".to_owned(),
            expires_at: "2100-01-01T00:00:00Z".to_owned(),
        };
        let error = render("preview-budget", &boundary, &draft, &registry, 0)
            .expect_err("mandatory context must not be trimmed to fit");
        assert_eq!(error, "mandatory_budget_exceeded");
    }

    #[test]
    fn prepared_material_binds_boundary_configuration_and_excludes_provider_context() {
        let plane = ControlPlane::synthetic();
        let boundary = plane.state().boundary.expect("synthetic boundary");
        let scope = boundary.scope.clone();
        let content = b"retained history".to_vec();
        let item = item_ref("history-render", 1, &content);
        let mut registry = BTreeMap::new();
        registry.insert(
            (item.item_id.clone(), item.version),
            record(
                item.clone(),
                scope.clone(),
                "history",
                false,
                content.clone(),
                4_102_444_800,
            ),
        );

        let material = render(
            "preview-render-contract",
            &boundary,
            &draft(scope.clone(), vec![item.clone()]),
            &registry,
            1_788_998_400,
        )
        .expect("prepared material");
        let input: Value = serde_json::from_slice(&material.input).expect("prepared input JSON");
        assert_eq!(
            input["schema"],
            "ascension.context-control.prepared-input.v1"
        );
        assert_eq!(input["profile"], "management-enabled");
        assert_eq!(
            input["scope"],
            serde_json::to_value(&scope).expect("scope JSON")
        );
        assert_eq!(input["protected_state"]["state_id"], boundary.state_id);
        assert_eq!(input["protected_state"]["generation"], boundary.generation);
        assert_eq!(
            input["protected_state"]["observation_sha256"],
            boundary.observation_sha256
        );
        assert_eq!(
            input["protected_state"]["catalog_sha256"],
            boundary.catalog_sha256
        );
        assert_eq!(input["protected_state"]["authority"], "harness-owned");
        assert_eq!(input["selected_items"][0]["item_id"], item.item_id);
        assert_eq!(input["selected_items"][0]["content"], "retained history");
        let serialized = String::from_utf8(material.input.clone()).expect("UTF-8 input");
        assert!(
            serialized.find("protected_state").expect("protected state")
                < serialized.find("selected_items").expect("selected items")
        );

        let configuration: Value =
            serde_json::from_slice(&material.configuration).expect("configuration JSON");
        assert_eq!(configuration["adapter_revision"], boundary.adapter_revision);
        assert_eq!(configuration["model"], boundary.model);
        assert_eq!(configuration["provider_added_context"], "not_exposed");
        assert_eq!(configuration["tools"], serde_json::json!([]));
        assert_eq!(configuration["credentials"], "excluded");
        assert_eq!(material.schema.as_slice(), output_schema());
        assert_eq!(
            material
                .components
                .iter()
                .map(|component| component.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["stdin", "output_schema", "configuration"]
        );
        assert!(
            material
                .components
                .iter()
                .skip(1)
                .all(|component| component.protected)
        );
    }

    #[test]
    fn render_rejects_unknown_protected_expired_and_non_utf8_content() {
        let plane = ControlPlane::synthetic();
        let boundary = plane.state().boundary.expect("synthetic boundary");
        let scope = boundary.scope.clone();
        let unknown = item_ref("unknown-render", 1, b"unknown");
        let empty_registry = BTreeMap::new();
        assert_eq!(
            render(
                "preview-unknown",
                &boundary,
                &draft(scope.clone(), vec![unknown]),
                &empty_registry,
                1_788_998_400,
            )
            .expect_err("unknown content must fail closed"),
            "unknown item unknown-render"
        );

        for (item_id, content, protected, expires_at, expected) in [
            (
                "protected-render",
                b"protected".to_vec(),
                true,
                4_102_444_800,
                "protected item protected-render cannot be rendered as an edit",
            ),
            (
                "expired-render",
                b"expired".to_vec(),
                false,
                946_684_800,
                "item expired-render is expired or unavailable",
            ),
            (
                "empty-render",
                Vec::new(),
                false,
                4_102_444_800,
                "item empty-render is expired or unavailable",
            ),
            (
                "invalid-utf8-render",
                vec![0xff, 0xfe],
                false,
                4_102_444_800,
                "item invalid-utf8-render is not UTF-8 text",
            ),
        ] {
            let item = item_ref(item_id, 1, &content);
            let mut registry = BTreeMap::new();
            registry.insert(
                (item.item_id.clone(), item.version),
                record(
                    item.clone(),
                    scope.clone(),
                    "history",
                    protected,
                    content,
                    expires_at,
                ),
            );
            assert_eq!(
                render(
                    "preview-invalid-content",
                    &boundary,
                    &draft(scope.clone(), vec![item]),
                    &registry,
                    1_788_998_400,
                )
                .expect_err("invalid content must fail closed"),
                expected
            );
        }
    }
}
