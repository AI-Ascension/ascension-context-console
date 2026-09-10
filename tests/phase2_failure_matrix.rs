// SPDX-License-Identifier: MIT

//! Row-level product assertions for the deterministic failure cases that the fixture can run.
//! The complete package matrix is recorded in `docs/evidence/phase2-failure-matrix-20260910.json`;
//! scenarios that require a live provider, durable storage, or process fault injection remain
//! explicitly unexecuted there.

use context_service::{ControlCommand, ControlOperation, ControlPatch, ControlPlane, ControlScope};

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

fn locked_item(plane: &ControlPlane) -> context_service::ControlItemRef {
    plane
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "locked-state")
        .expect("protected fixture")
        .item
}

fn draft(plane: &mut ControlPlane) -> context_service::ControlDraft {
    let active = plane.state().active_revision_id;
    plane
        .create_draft(scope(plane), &active, "operator-fixture")
        .expect("draft")
}

fn selected_draft(plane: &mut ControlPlane) -> context_service::ControlDraft {
    let created = draft(plane);
    plane
        .apply_patch(
            patch(
                plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem {
                    item: history_item(plane),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("select history")
}

fn applicable_preview(
    plane: &mut ControlPlane,
    draft: &context_service::ControlDraft,
    pause_key: &str,
) -> context_service::ControlPreview {
    let pause = command(plane, "pause", pause_key, plane.state().control_version);
    plane.pause(pause).expect("pause");
    plane
        .create_preview(
            scope(plane),
            &draft.draft_id,
            draft.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("applicable preview")
}

#[test]
fn p2_f013_two_operators_have_one_cas_winner() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let history = history_item(&plane);
    plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem {
                    item: history.clone(),
                }],
            ),
            "operator-a",
            false,
        )
        .expect("first operator wins");
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem { item: history }],
            ),
            "operator-b",
            false,
        )
        .expect_err("second operator must observe the stale draft");
    assert_eq!(error.code, "stale_draft");
    assert_eq!(
        plane.get_draft(&created.draft_id).expect("draft").version,
        2
    );
}

#[test]
fn p2_f008_permission_revoked_after_preview_blocks_commit() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-permission-preview");
    plane.deactivate();
    let mut commit = command(
        &plane,
        "commit",
        "commit-permission-preview",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    assert_eq!(
        plane
            .commit(commit)
            .expect_err("revoked management permission must block commit")
            .code,
        "management_disabled"
    );
    assert_eq!(plane.state().active_revision_id, "revision-1");
    assert!(plane.state().pause_latched);
}

#[test]
fn p2_f009_permission_revoked_after_commit_blocks_resume() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-permission-commit");
    let mut commit = command(
        &plane,
        "commit",
        "commit-permission-commit",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    plane.commit(commit).expect("commit before revocation");
    plane.deactivate();

    let mut resume = command(
        &plane,
        "resume",
        "resume-permission-commit",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(plane.state().active_revision_id);
    resume.expected_preview_id = Some(preview.preview_id);
    assert_eq!(
        plane
            .resume(resume)
            .expect_err("revoked management permission must block resume")
            .code,
        "management_disabled"
    );
    assert!(plane.state().pause_latched);
    assert_eq!(plane.state().status, "paused_committed");
}

#[test]
fn p2_f014_two_previews_have_one_serialized_commit_winner() {
    let mut plane = ControlPlane::synthetic();
    let first = selected_draft(&mut plane);
    let second_created = draft(&mut plane);
    let second = plane
        .apply_patch(
            patch(
                &plane,
                &second_created.draft_id,
                second_created.version,
                vec![
                    ControlOperation::IncludeItem {
                        item: history_item(&plane),
                    },
                    ControlOperation::PutNote {
                        note_id: "operator-b-note".to_owned(),
                        expected_note_version: None,
                        text: "second operator configuration".to_owned(),
                        expires_at: "2030-01-01T00:00:00Z".to_owned(),
                    },
                ],
            ),
            "operator-b",
            false,
        )
        .expect("second operator draft");
    let pause = command(
        &plane,
        "pause",
        "pause-two-previews",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    let first_preview = plane
        .create_preview(
            scope(&plane),
            &first.draft_id,
            first.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("first preview");
    let second_preview = plane
        .create_preview(
            scope(&plane),
            &second.draft_id,
            second.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("second preview");
    let expected_control_version = plane.state().control_version;
    let expected_revision = plane.state().active_revision_id.clone();
    let mut first_commit = command(
        &plane,
        "commit",
        "commit-two-previews-a",
        expected_control_version,
    );
    first_commit.expected_active_revision_id = Some(expected_revision.clone());
    first_commit.preview_id = Some(first_preview.preview_id);
    first_commit.approved_manifest_sha256 = first_preview.prepared_manifest_sha256;
    let mut second_commit = command(
        &plane,
        "commit",
        "commit-two-previews-b",
        expected_control_version,
    );
    second_commit.expected_active_revision_id = Some(expected_revision);
    second_commit.preview_id = Some(second_preview.preview_id);
    second_commit.approved_manifest_sha256 = second_preview.prepared_manifest_sha256;

    plane.commit(first_commit).expect("first serialized winner");
    assert_eq!(
        plane
            .commit(second_commit)
            .expect_err("second stale operator must lose")
            .code,
        "stale_revision"
    );
    assert_ne!(plane.state().active_revision_id, "revision-1");
}

#[test]
fn p2_f045_host_evolution_while_held_marks_the_boundary_stale() {
    let mut plane = ControlPlane::synthetic();
    let pause = command(
        &plane,
        "pause",
        "pause-host-evolution",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    assert_eq!(plane.state().status, "paused_ready");
    plane.advance_boundary();
    assert_eq!(plane.state().status, "paused_stale");
    let created = draft(&mut plane);
    let preview = plane
        .create_preview(
            scope(&plane),
            &created.draft_id,
            created.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("stale held preview");
    assert!(!preview.applicable);
    assert_eq!(preview.blockers, vec!["run_not_held"]);
}

#[test]
fn p2_f052_concurrent_resume_claims_have_one_winner() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-concurrent-resume");
    let mut commit = command(
        &plane,
        "commit",
        "commit-concurrent-resume",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    plane.commit(commit).expect("commit");

    let expected_control_version = plane.state().control_version;
    let expected_revision = plane.state().active_revision_id.clone();
    let mut first = command(
        &plane,
        "resume",
        "resume-concurrent-a",
        expected_control_version,
    );
    first.expected_active_revision_id = Some(expected_revision.clone());
    first.expected_preview_id = Some(preview.preview_id.clone());
    let mut second = command(
        &plane,
        "resume",
        "resume-concurrent-b",
        expected_control_version,
    );
    second.expected_active_revision_id = Some(expected_revision);
    second.expected_preview_id = Some(preview.preview_id);

    plane.resume(first).expect("first resume claim");
    assert_eq!(
        plane
            .resume(second)
            .expect_err("second concurrent claim must lose")
            .code,
        "stale_control"
    );
    assert_eq!(
        plane
            .events()
            .iter()
            .filter(|event| event.event_type == "input.submitted")
            .count(),
        1
    );
}

#[test]
fn p2_f058_no_edit_resume_after_drain_releases_without_new_input() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-no-edit-drain");
    let mut commit = command(
        &plane,
        "commit",
        "commit-no-edit-drain",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let receipt = plane.commit(commit).expect("no-edit commit");
    assert_eq!(receipt.effect, "no_change");
    assert!(plane.prepared_input().is_none());

    let mut resume = command(
        &plane,
        "resume",
        "resume-no-edit-drain",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(plane.state().active_revision_id);
    resume.expected_preview_id = Some(preview.preview_id);
    plane.resume(resume).expect("no-edit resume");
    assert_eq!(
        plane
            .events()
            .iter()
            .filter(|event| event.event_type == "input.submitted")
            .count(),
        0
    );
}

#[test]
fn p2_f015_protected_item_cannot_be_excluded_or_selected() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem {
                    item: locked_item(&plane),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect_err("protected item must remain host-owned");
    assert_eq!(error.code, "protected_item");
    assert_eq!(
        plane.get_draft(&created.draft_id).expect("draft").version,
        1
    );
}

#[test]
fn p2_f016_note_identity_cannot_alias_protected_item() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::PutNote {
                    note_id: "locked-state".to_owned(),
                    expected_note_version: None,
                    text: "forged authority".to_owned(),
                    expires_at: "2030-01-01T00:00:00Z".to_owned(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect_err("note must not shadow protected identity");
    assert_eq!(error.code, "protected_item");
}

#[test]
fn p2_f020_pin_requires_selection_and_exclude_requires_unpin() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let history = history_item(&plane);
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::PinItem {
                    item: history.clone(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect_err("pinning must not select implicitly");
    assert_eq!(error.code, "pin_requires_selection");
    let selected = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem {
                    item: history.clone(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("select history");
    let pinned = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                selected.version,
                vec![ControlOperation::PinItem { item: history }],
            ),
            "operator-fixture",
            false,
        )
        .expect("pin history");
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                pinned.version,
                vec![ControlOperation::ExcludeItem {
                    item: history_item(&plane),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect_err("pinned item must be unpinned before exclusion");
    assert_eq!(error.code, "pinned_item");
}

#[test]
fn p2_f022_restore_is_a_fresh_configuration_draft() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let restored = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::RestoreConfiguration {
                    source_revision_id: "revision-1".to_owned(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("restore");
    assert_eq!(restored.base_revision_id, "revision-1");
    assert_ne!(restored.draft_id, "revision-1");
    assert!(restored.selected_items.is_empty());
    assert!(restored.pinned_item_ids.is_empty());
}

#[test]
fn p2_f023_restore_cannot_be_combined_with_other_operations() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![
                    ControlOperation::RestoreConfiguration {
                        source_revision_id: "revision-1".to_owned(),
                    },
                    ControlOperation::IncludeItem {
                        item: history_item(&plane),
                    },
                ],
            ),
            "operator-fixture",
            false,
        )
        .expect_err("restore must be an exclusive operation");
    assert_eq!(error.code, "restore_must_be_alone");
}

#[test]
fn p2_f024_objective_requires_elevated_authorization() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::SetObjective {
                    text: "operator objective".to_owned(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect_err("objective writes require the separate capability");
    assert_eq!(error.code, "objective_authorization_required");
}

#[test]
fn p2_f025_identical_configuration_is_a_no_change_commit() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-no-change-matrix");
    let before = plane.state();
    let mut commit = command(
        &plane,
        "commit",
        "commit-no-change-matrix",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let receipt = plane.commit(commit).expect("no-change commit");
    assert_eq!(receipt.effect, "no_change");
    assert_eq!(plane.state().active_revision_id, before.active_revision_id);
    assert_eq!(plane.state().plan_epoch, before.plan_epoch);
    assert_eq!(plane.state().control_version, before.control_version);
}

#[test]
fn p2_f026_running_preview_is_exploratory_only() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = plane
        .create_preview(
            scope(&plane),
            &created.draft_id,
            created.version,
            true,
            plane.state().control_version,
            false,
        )
        .expect("blocked preview is still inspectable");
    assert!(!preview.applicable);
    assert_eq!(preview.blockers, vec!["run_not_held"]);
    assert_eq!(plane.state().status, "running");
}

#[test]
fn p2_f027_boundary_change_invalidates_commit() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-boundary-matrix");
    plane.advance_boundary();
    let mut commit = command(
        &plane,
        "commit",
        "commit-boundary-matrix",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let error = plane
        .commit(commit)
        .expect_err("boundary change must stale preview");
    assert_eq!(error.code, "preview_stale");
    assert_eq!(plane.state().active_revision_id, "revision-1");
}

#[test]
fn p2_f028_catalog_bytes_change_with_same_state_label_stales_preview() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-catalog-matrix");
    let before = plane.state();
    plane.advance_catalog();
    let after = plane.state();
    assert_eq!(
        before.boundary.as_ref().unwrap().state_id,
        after.boundary.as_ref().unwrap().state_id
    );
    assert_ne!(
        before.boundary.as_ref().unwrap().catalog_sha256,
        after.boundary.as_ref().unwrap().catalog_sha256
    );
    let mut commit = command(
        &plane,
        "commit",
        "commit-catalog-matrix",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    assert_eq!(
        plane
            .commit(commit)
            .expect_err("catalog digest changed")
            .code,
        "preview_stale"
    );
    assert_eq!(plane.state().active_revision_id, "revision-1");
}

#[test]
fn p2_f029_provider_fingerprint_change_stales_preview() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-provider-fingerprint");
    let before = plane.state();
    plane.advance_provider_fingerprint();
    let after = plane.state();
    assert_ne!(
        before.boundary.as_ref().unwrap().adapter_revision,
        after.boundary.as_ref().unwrap().adapter_revision
    );
    assert_ne!(
        before.boundary.as_ref().unwrap().configuration_sha256,
        after.boundary.as_ref().unwrap().configuration_sha256
    );
    assert_ne!(
        before.boundary.as_ref().unwrap().model,
        after.boundary.as_ref().unwrap().model
    );
    let mut commit = command(
        &plane,
        "commit",
        "commit-provider-fingerprint",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    assert_eq!(
        plane
            .commit(commit)
            .expect_err("provider fingerprint changed")
            .code,
        "preview_stale"
    );
    assert_eq!(plane.state().active_revision_id, "revision-1");
}

#[test]
fn p2_f035_draft_change_invalidates_previous_preview() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-draft-matrix");
    let changed = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::UnpinItem {
                    item: history_item(&plane),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("draft edit");
    assert_eq!(changed.version, created.version + 1);
    let mut commit = command(
        &plane,
        "commit",
        "commit-draft-matrix",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let error = plane
        .commit(commit)
        .expect_err("old preview must not be reusable");
    assert_eq!(error.code, "preview_stale");
}

#[test]
fn p2_f036_expired_preview_cannot_be_committed() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-expiry-matrix");
    plane.advance_time(121);
    let mut commit = command(
        &plane,
        "commit",
        "commit-expiry-matrix",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let error = plane
        .commit(commit)
        .expect_err("expired preview must be rejected");
    assert_eq!(error.code, "preview_stale");
}

#[test]
fn p2_f046_old_pause_replay_returns_original_receipt() {
    let mut plane = ControlPlane::synthetic();
    let pause = command(
        &plane,
        "pause",
        "pause-replay-matrix",
        plane.state().control_version,
    );
    let receipt = plane.pause(pause.clone()).expect("pause");
    let resume = command(
        &plane,
        "resume",
        "resume-replay-matrix",
        plane.state().control_version,
    );
    plane.resume(resume).expect("resume");
    let replay = plane.pause(pause).expect("same-key replay is idempotent");
    assert_eq!(replay, receipt);
    assert!(!plane.state().pause_latched);
}

#[test]
fn p2_f051_old_plan_epoch_is_rejected_after_commit() {
    let mut plane = ControlPlane::synthetic();
    let created = selected_draft(&mut plane);
    let preview = applicable_preview(&mut plane, &created, "pause-plan-matrix");
    let old_epoch = plane.state().plan_epoch;
    let mut commit = command(
        &plane,
        "commit",
        "commit-plan-matrix",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    plane.commit(commit).expect("commit");
    let error = plane
        .validate_plan_epoch(old_epoch)
        .expect_err("old plan must be fenced");
    assert_eq!(error.code, "obsolete_plan");
}

#[test]
fn p2_f053_changed_body_reuse_is_an_idempotency_conflict() {
    let mut plane = ControlPlane::synthetic();
    let pause = command(
        &plane,
        "pause",
        "pause-idempotency-matrix",
        plane.state().control_version,
    );
    plane.pause(pause.clone()).expect("pause");
    let mut changed = pause;
    changed.expected_control_version = plane.state().control_version;
    let error = plane
        .pause(changed)
        .expect_err("changed body must not reuse a key");
    assert_eq!(error.code, "idempotency_conflict");
}

#[test]
fn p2_f055_stop_dominates_resume() {
    let mut plane = ControlPlane::synthetic();
    let revision = plane.state().active_revision_id;
    plane.stop();
    let resume = command(
        &plane,
        "resume",
        "resume-stop-matrix",
        plane.state().control_version,
    );
    let error = plane.resume(resume).expect_err("stop must remain dominant");
    assert_eq!(error.code, "stopped");
    assert_eq!(plane.state().active_revision_id, revision);
}

#[test]
fn p2_f066_deactivation_preserves_pause_and_revision() {
    let mut plane = ControlPlane::synthetic();
    let revision = plane.state().active_revision_id;
    let pause = command(
        &plane,
        "pause",
        "pause-deactivate-matrix",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    plane.deactivate();
    assert!(!plane.enabled());
    assert_eq!(plane.state().active_revision_id, revision);
    assert!(plane.state().pause_latched);
    let error = plane
        .create_draft(scope(&plane), &revision, "operator-fixture")
        .expect_err("deactivated writes must fail closed");
    assert_eq!(error.code, "management_disabled");
}
