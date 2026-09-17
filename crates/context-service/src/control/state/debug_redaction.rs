// SPDX-License-Identifier: MIT

// `ControlPlane` carries `items`, a map that is `#[serde(skip)]` because `ItemRecord::content` is
// renderer-private text documented as never appearing in control errors or telemetry; the plane also
// holds `prepared_input` and draft/preview `PreparedMaterial` bytes. A serde skip governs
// serialization only, so a derived `Debug` printed all of it through `{:?}` and `{:#?}`. The impls
// below are allowlisted: identity and shape metadata only, bytes as lengths and collections as
// counts, and every impl ends with `finish_non_exhaustive()` so a future sensitive field cannot
// silently re-enter the format path. They live in this child module because they read the private
// fields of the records declared in `state/mod.rs`.

use super::*;

impl std::fmt::Debug for ControlPlane {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ControlPlane")
            .field("scope", &self.scope)
            .field("enabled", &self.enabled)
            .field("now", &self.now)
            .field("boundary", &self.boundary)
            .field("state", &self.state)
            .field("item_count", &self.items.len())
            .field("latest_version_count", &self.latest_versions.len())
            .field("draft_count", &self.drafts.len())
            .field("revision_count", &self.revisions.len())
            .field("preview_count", &self.previews.len())
            .field("command_count", &self.commands.len())
            .field("event_count", &self.events.len())
            .field("relation_count", &self.relations.len())
            .field("next_id", &self.next_id)
            .field("continuation_preview_id", &self.continuation_preview_id)
            .field(
                "prepared_input_len",
                &self.prepared_input.as_ref().map_or(0, Vec::len),
            )
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ItemRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ItemRecord")
            .field("item", &self.item)
            .field("kind", &self.kind)
            .field("protected", &self.protected)
            .field("scope", &self.scope)
            .field("content_len", &self.content.len())
            .field("expires_at", &self.expires_at)
            .field("expires_text", &self.expires_text)
            .field("locked_reason", &self.locked_reason)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for PreparedMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedMaterial")
            .field("input_len", &self.input.len())
            .field("schema_len", &self.schema.len())
            .field("configuration_len", &self.configuration.len())
            .field("component_count", &self.components.len())
            .field("manifest_sha256", &self.manifest_sha256)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for DraftRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DraftRecord")
            .field("draft", &self.draft)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for PreviewRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreviewRecord")
            .field("preview", &self.preview)
            .field("material", &self.material)
            .field("consumed", &self.consumed)
            .finish_non_exhaustive()
    }
}
