// SPDX-License-Identifier: MIT

use context_service::{
    ControlCommand, ControlError, ControlOperation, ControlPatch, ControlPlane, ControlScope,
};

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

#[test]
fn protected_edit_and_draft_cas_fail_without_mutating_the_draft() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let locked = plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "locked-state")
        .expect("protected fixture")
        .item;
    let rejected = plane.apply_patch(
        patch(
            &plane,
            &created.draft_id,
            created.version,
            vec![ControlOperation::IncludeItem { item: locked }],
        ),
        "operator-fixture",
        false,
    );
    assert_eq!(
        rejected.expect_err("protected edit must fail").code,
        "protected_item"
    );
    assert_eq!(
        plane.get_draft(&created.draft_id).expect("draft").version,
        1
    );

    let history = history_item(&plane);
    let saved = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                1,
                vec![ControlOperation::IncludeItem {
                    item: history.clone(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("editable item");
    assert_eq!(saved.version, 2);
    let stale = plane.apply_patch(
        patch(
            &plane,
            &created.draft_id,
            1,
            vec![ControlOperation::IncludeItem { item: history }],
        ),
        "operator-fixture",
        false,
    );
    assert_eq!(
        stale.expect_err("stale draft must fail").code,
        "stale_draft"
    );
    assert_eq!(
        plane
            .get_draft(&created.draft_id)
            .expect("draft")
            .selected_items
            .len(),
        1
    );
}

#[test]
fn objective_authorization_and_restore_alone_are_enforced() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let objective = plane.apply_patch(
        patch(
            &plane,
            &created.draft_id,
            1,
            vec![ControlOperation::SetObjective {
                text: "operator objective".to_owned(),
            }],
        ),
        "operator-fixture",
        false,
    );
    assert_eq!(
        objective
            .expect_err("objective override requires explicit authorization")
            .code,
        "objective_authorization_required"
    );
    let mixed_restore = plane.apply_patch(
        patch(
            &plane,
            &created.draft_id,
            1,
            vec![
                ControlOperation::RestoreConfiguration {
                    source_revision_id: "revision-1".to_owned(),
                },
                ControlOperation::SetObjective {
                    text: "objective".to_owned(),
                },
            ],
        ),
        "objective-fixture",
        true,
    );
    assert_eq!(
        mixed_restore.expect_err("restore is exclusive").code,
        "restore_must_be_alone"
    );
}

#[test]
fn preview_pause_commit_resume_fences_the_old_plan_and_reuses_receipts() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let history = history_item(&plane);
    let edited = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                1,
                vec![ControlOperation::IncludeItem { item: history }],
            ),
            "operator-fixture",
            false,
        )
        .expect("edit");
    let exploratory = plane
        .create_preview(
            scope(&plane),
            &edited.draft_id,
            edited.version,
            false,
            plane.state().control_version,
            false,
        )
        .expect("exploratory preview");
    assert!(!exploratory.applicable);
    assert!(exploratory.blockers.is_empty());
    assert_eq!(exploratory.provider_added_context, "not_exposed");

    let pause_command = command(&plane, "pause", "pause-1", plane.state().control_version);
    let paused = plane.pause(pause_command.clone()).expect("pause");
    assert_eq!(paused.effect, "pause_requested");
    let duplicate_pause = plane.pause(pause_command).expect("idempotent pause");
    assert_eq!(duplicate_pause, paused);

    let applicable = plane
        .create_preview(
            scope(&plane),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("applicable preview");
    assert!(applicable.applicable);
    let old_plan_epoch = plane.state().plan_epoch;
    let mut commit_command = command(&plane, "commit", "commit-1", plane.state().control_version);
    commit_command.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit_command.preview_id = Some(applicable.preview_id.clone());
    commit_command.approved_manifest_sha256 = applicable.prepared_manifest_sha256.clone();
    let committed = plane.commit(commit_command.clone()).expect("commit");
    assert_eq!(committed.effect, "revision_committed");
    assert!(plane.state().pause_latched);
    assert_eq!(plane.state().status, "paused_committed");
    assert!(plane.prepared_input().is_some());
    assert_eq!(
        plane
            .validate_plan_epoch(old_plan_epoch)
            .expect_err("old plan fenced")
            .code,
        "obsolete_plan"
    );
    assert_eq!(
        plane
            .commit(commit_command.clone())
            .expect("idempotent commit"),
        committed
    );
    let mut changed_commit = commit_command.clone();
    changed_commit.approved_manifest_sha256 = Some("0".repeat(64));
    assert_eq!(
        plane
            .commit(changed_commit)
            .expect_err("changed command body conflicts")
            .code,
        "idempotency_conflict"
    );

    let mut resume_command = command(&plane, "resume", "resume-1", plane.state().control_version);
    resume_command.expected_active_revision_id = Some(plane.state().active_revision_id);
    resume_command.expected_preview_id = Some(applicable.preview_id);
    let resumed = plane.resume(resume_command.clone()).expect("resume");
    assert_eq!(resumed.effect, "resume_accepted");
    assert!(!plane.state().pause_latched);
    assert_eq!(
        plane.resume(resume_command).expect("idempotent resume"),
        resumed
    );
    assert!(plane.validate_plan_epoch(plane.state().plan_epoch).is_ok());
}

#[test]
fn boundary_change_rejects_a_committed_continuation() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let history = history_item(&plane);
    let edited = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                1,
                vec![ControlOperation::IncludeItem { item: history }],
            ),
            "operator-fixture",
            false,
        )
        .expect("edit");
    let pause = command(
        &plane,
        "pause",
        "pause-boundary",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    let preview = plane
        .create_preview(
            scope(&plane),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("preview");
    let mut commit = command(
        &plane,
        "commit",
        "commit-boundary",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256.clone();
    plane.commit(commit).expect("commit");
    plane.advance_boundary();
    let mut resume = command(
        &plane,
        "resume",
        "resume-boundary",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(plane.state().active_revision_id);
    resume.expected_preview_id = Some(preview.preview_id);
    let error: ControlError = plane
        .resume(resume)
        .expect_err("old continuation is fenced");
    assert_eq!(error.code, "preview_stale");
}

#[test]
fn journal_recovery_preserves_pause_and_fences_the_previous_controller() {
    let mut plane = ControlPlane::synthetic();
    let before_epoch = plane.state().controller_epoch;
    let command = command(
        &plane,
        "pause",
        "pause-recovery",
        plane.state().control_version,
    );
    let receipt = plane.pause(command.clone()).expect("pause");
    let journal = plane.export_journal().expect("journal");
    let mut recovered = ControlPlane::recover_journal(&journal).expect("recovery");
    assert!(recovered.state().pause_latched);
    assert_eq!(recovered.state().controller_epoch, before_epoch + 1);
    assert_eq!(
        recovered.pause(command).expect("idempotent replay"),
        receipt
    );
}

#[test]
fn disabled_profile_does_not_advertise_or_apply_management_operations() {
    let scope = ControlScope::new("project", "run", "episode", "agent").expect("scope");
    let mut plane = ControlPlane::new(scope.clone(), false, 1_788_998_400).expect("plane");
    assert!(!plane.capabilities().enabled);
    assert!(plane.capabilities().supported_operations.is_empty());
    assert_eq!(
        plane
            .create_draft(scope, "revision-1", "operator")
            .expect_err("disabled profile")
            .code,
        "management_disabled"
    );
}

#[test]
fn stop_latch_dominates_resume_without_changing_the_revision() {
    let mut plane = ControlPlane::synthetic();
    let revision = plane.state().active_revision_id;
    plane.stop();
    assert_eq!(plane.state().status, "stopped");
    assert_eq!(plane.state().active_revision_id, revision);
    let resume = command(
        &plane,
        "resume",
        "resume-stopped",
        plane.state().control_version,
    );
    assert_eq!(
        plane.resume(resume).expect_err("stopped run").code,
        "stopped"
    );
}
