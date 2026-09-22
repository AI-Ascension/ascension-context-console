// SPDX-License-Identifier: MIT


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
fn capability_projection_does_not_claim_production_durability() {
    let plane = ControlPlane::synthetic();
    let capabilities = plane.capabilities();
    assert!(capabilities.enabled);
    assert_eq!(capabilities.durable_control_store, "unverified");
    assert!(!capabilities.commit_auto_resumes);
    assert!(!capabilities.direct_game_dispatch);
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

#[test]
fn identical_configuration_commit_returns_no_change_without_epoch_churn() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let pause = command(
        &plane,
        "pause",
        "pause-no-change",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    let preview = plane
        .create_preview(
            scope(&plane),
            &created.draft_id,
            created.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("preview");
    let before_revision = plane.state().active_revision_id.clone();
    let before_plan_epoch = plane.state().plan_epoch;
    let before_control_version = plane.state().control_version;
    let mut commit = command(
        &plane,
        "commit",
        "commit-no-change",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(before_revision.clone());
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256.clone();
    let receipt = plane.commit(commit).expect("no-change commit");
    assert_eq!(receipt.effect, "no_change");
    assert_eq!(plane.state().active_revision_id, before_revision);
    assert_eq!(plane.state().plan_epoch, before_plan_epoch);
    assert_eq!(plane.state().control_version, before_control_version);
    assert_eq!(plane.state().status, "paused_ready");
    assert!(plane.prepared_input().is_none());
    let mut resume = command(
        &plane,
        "resume",
        "resume-no-change",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(before_revision);
    resume.expected_preview_id = Some(preview.preview_id);
    assert_eq!(
        plane.resume(resume).expect("resume after no-change").effect,
        "resume_accepted"
    );
}
