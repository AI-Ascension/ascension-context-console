// SPDX-License-Identifier: MIT

//! `demo`, `integrated-demo`, and `phase2-demo` dispatch for the `context-console` CLI.

use context_service::{ControlCommand, ControlOperation, ControlPatch, ControlPlane};

pub(super) fn run_demo() -> Result<(), String> {
    context_service::demo().map_err(|error| error.to_string())
}

pub(super) fn run_integrated_demo(port: u16) -> Result<(), String> {
    context_service::run_integrated_demo(port)
}

pub(super) fn run_phase2_demo() -> Result<(), String> {
    let mut plane = ControlPlane::synthetic();
    let scope = plane.scope().clone();
    let initial = plane.state();
    let draft = plane
        .create_draft(
            scope.clone(),
            &initial.active_revision_id,
            "operator-fixture",
        )
        .map_err(|error| error.to_string())?;
    let history = plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .ok_or_else(|| "history fixture is unavailable".to_owned())?
        .item;
    let edited = plane
        .apply_patch(
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id.clone(),
                expected_draft_version: draft.version,
                expected_active_revision_id: initial.active_revision_id,
                operations: vec![ControlOperation::IncludeItem { item: history }],
            },
            "operator-fixture",
            false,
        )
        .map_err(|error| error.to_string())?;
    let exploratory = plane
        .create_preview(
            scope.clone(),
            &edited.draft_id,
            edited.version,
            false,
            plane.state().control_version,
            false,
        )
        .map_err(|error| error.to_string())?;
    let pause = command(&plane, "pause", "phase2-pause", None, None);
    let paused = plane.pause(pause).map_err(|error| error.to_string())?;
    let applicable = plane
        .create_preview(
            scope.clone(),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .map_err(|error| error.to_string())?;
    let mut commit = command(
        &plane,
        "commit",
        "phase2-commit",
        Some(plane.state().active_revision_id.clone()),
        Some(applicable.preview_id.clone()),
    );
    commit.approved_manifest_sha256 = applicable.prepared_manifest_sha256.clone();
    let committed = plane.commit(commit).map_err(|error| error.to_string())?;
    let mut resume = command(
        &plane,
        "resume",
        "phase2-resume",
        Some(plane.state().active_revision_id.clone()),
        None,
    );
    resume.expected_preview_id = Some(applicable.preview_id.clone());
    let resumed = plane.resume(resume).map_err(|error| error.to_string())?;
    println!("phase2_demo=true");
    println!("scope_run_id={}", scope.run_id);
    println!("draft_version={}", edited.version);
    println!("exploratory_applicable={}", exploratory.applicable);
    println!("pause_effect={}", paused.effect);
    println!("preview_applicable={}", applicable.applicable);
    println!(
        "prepared_manifest_sha256={}",
        applicable.prepared_manifest_sha256.unwrap_or_default()
    );
    println!("commit_effect={}", committed.effect);
    println!("resume_effect={}", resumed.effect);
    println!("provider_calls=0");
    println!("game_launches=0");
    Ok(())
}

fn command(
    plane: &ControlPlane,
    kind: &str,
    idempotency_key: &str,
    expected_active_revision_id: Option<String>,
    expected_preview_id: Option<String>,
) -> ControlCommand {
    ControlCommand {
        schema: "ascension.context-control.command.v1".to_owned(),
        scope: plane.scope().clone(),
        idempotency_key: idempotency_key.to_owned(),
        command_window_id: plane.state().command_window_id,
        expected_control_version: plane.state().control_version,
        kind: kind.to_owned(),
        expected_active_revision_id,
        preview_id: expected_preview_id.clone(),
        approved_manifest_sha256: None,
        expected_preview_id,
    }
}
