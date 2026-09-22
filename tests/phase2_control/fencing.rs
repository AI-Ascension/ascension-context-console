// SPDX-License-Identifier: MIT


#[test]
fn expired_items_and_previews_cannot_be_approved_or_committed() {
    let expired_scope = ControlScope::new("project", "run", "episode", "agent").expect("scope");
    let mut plane = ControlPlane::new(expired_scope, true, 4_102_444_700).expect("plane");
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
        "pause-expiry",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    plane.advance_time(101);
    let expired = plane
        .create_preview(
            scope(&plane),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("blocked expired preview");
    assert!(!expired.applicable);
    assert_eq!(expired.blockers, vec!["mandatory_budget_exceeded"]);

    let mut fresh = ControlPlane::synthetic();
    let created = draft(&mut fresh);
    let pause = command(
        &fresh,
        "pause",
        "pause-preview-expiry",
        fresh.state().control_version,
    );
    fresh.pause(pause).expect("pause");
    let preview = fresh
        .create_preview(
            scope(&fresh),
            &created.draft_id,
            created.version,
            true,
            fresh.state().control_version,
            true,
        )
        .expect("preview");
    fresh.advance_time(121);
    let mut commit = command(
        &fresh,
        "commit",
        "commit-preview-expiry",
        fresh.state().control_version,
    );
    commit.expected_active_revision_id = Some(fresh.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id);
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    assert_eq!(
        fresh.commit(commit).expect_err("expired preview").code,
        "preview_stale"
    );
}

#[test]
fn safe_deactivation_preserves_revision_pause_and_journal() {
    let mut plane = ControlPlane::synthetic();
    let revision = plane.state().active_revision_id.clone();
    let pause = command(
        &plane,
        "pause",
        "pause-deactivation",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    let before = plane.state();
    plane.deactivate();
    assert!(!plane.enabled());
    assert_eq!(plane.state().active_revision_id, revision);
    assert_eq!(plane.state().pause_latched, before.pause_latched);
    assert_eq!(plane.state().plan_epoch, before.plan_epoch);
    assert_eq!(
        plane
            .create_draft(scope(&plane), &revision, "operator-fixture")
            .expect_err("deactivated management writes")
            .code,
        "management_disabled"
    );
    let journal = plane.export_journal().expect("journal after deactivation");
    let recovered = ControlPlane::recover_journal(&journal).expect("deactivation recovery");
    assert!(!recovered.enabled());
    assert!(recovered.state().pause_latched);
    assert_eq!(recovered.state().active_revision_id, revision);
}

#[test]
fn expired_command_window_rejects_replay_without_mutation() {
    let mut plane = ControlPlane::synthetic();
    plane.advance_time(3_601);
    let command = command(
        &plane,
        "pause",
        "pause-expired-window",
        plane.state().control_version,
    );
    assert_eq!(
        plane
            .pause(command)
            .expect_err("expired command window")
            .code,
        "command_window_expired"
    );
    assert_eq!(plane.state().status, "running");
    assert!(!plane.state().pause_latched);
}

#[test]
fn p2_f039_resume_cannot_bypass_the_approved_preview_identity() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let created = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem {
                    item: history_item(&plane),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("select immutable history");
    let pause = command(
        &plane,
        "pause",
        "pause-preview-required",
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
    let mut commit = command(
        &plane,
        "commit",
        "commit-preview-required",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    plane.commit(commit).expect("commit");

    let mut resume = command(
        &plane,
        "resume",
        "resume-preview-required",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(plane.state().active_revision_id);
    assert_eq!(
        plane
            .resume(resume)
            .expect_err("resume without an approved preview must fail")
            .code,
        "preview_required"
    );
    assert!(plane.state().pause_latched);
    assert!(plane.prepared_input().is_some());
}
