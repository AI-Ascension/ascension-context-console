// SPDX-License-Identifier: MIT

use context_service::{
    ControlCommand, ControlError, ControlOperation, ControlPatch, ControlPlane, ControlScope,
};
use serde_json::Value;

const COMMAND_SCHEMA: &str = "ascension.context-control.command.v1";
const PATCH_SCHEMA: &str = "ascension.context-control.patch.v1";

fn scope(plane: &ControlPlane) -> ControlScope {
    plane.scope().clone()
}

fn command(
    plane: &ControlPlane,
    kind: &str,
    key: &str,
    expected_control_version: u64,
) -> ControlCommand {
    ControlCommand {
        schema: COMMAND_SCHEMA.to_owned(),
        scope: scope(plane),
        idempotency_key: key.to_owned(),
        command_window_id: plane.state().command_window_id,
        expected_control_version,
        kind: kind.to_owned(),
        expected_active_revision_id: None,
        preview_id: None,
        approved_manifest_sha256: None,
        expected_preview_id: None,
    }
}

fn patch(
    plane: &ControlPlane,
    draft_id: &str,
    version: u64,
    operations: Vec<ControlOperation>,
) -> ControlPatch {
    ControlPatch {
        schema: PATCH_SCHEMA.to_owned(),
        scope: scope(plane),
        draft_id: draft_id.to_owned(),
        expected_draft_version: version,
        expected_active_revision_id: plane.state().active_revision_id,
        operations,
    }
}

fn history_item(plane: &ControlPlane) -> context_service::ControlItemRef {
    plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .expect("history fixture")
        .item
}

fn draft(plane: &mut ControlPlane) -> context_service::ControlDraft {
    let active = plane.state().active_revision_id;
    plane
        .create_draft(scope(plane), &active, "operator-fixture")
        .expect("draft")
}

// The scenario groups below are spliced into THIS module rather than declared as
// submodules, so every discovered test keeps its original `phase2_control::<fn>`
// name: a real submodule would insert a path segment and rename all of them.
include!("phase2_control/draft_validation.rs");
include!("phase2_control/preview.rs");
include!("phase2_control/recovery.rs");
include!("phase2_control/fencing.rs");
