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
    let protected_alias = plane.apply_patch(
        patch(
            &plane,
            &created.draft_id,
            1,
            vec![ControlOperation::PutNote {
                note_id: "locked-state".to_owned(),
                expected_note_version: None,
                text: "alias".to_owned(),
                expires_at: "2030-01-01T00:00:00Z".to_owned(),
            }],
        ),
        "operator-fixture",
        false,
    );
    assert_eq!(
        protected_alias
            .expect_err("notes cannot alias protected identities")
            .code,
        "protected_item"
    );
}

#[test]
fn pinning_requires_selection_and_restore_is_a_new_configuration_draft() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let history = history_item(&plane);
    let pinned = plane
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
        .expect_err("pinning an unselected item must fail");
    assert_eq!(pinned.code, "pin_requires_selection");

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
        .expect("pin selected item");
    let excluded = plane.apply_patch(
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
    );
    assert_eq!(
        excluded
            .expect_err("pinned item must be unpinned first")
            .code,
        "pinned_item"
    );

    let restored = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                pinned.version,
                vec![ControlOperation::RestoreConfiguration {
                    source_revision_id: "revision-1".to_owned(),
                }],
            ),
            "operator-fixture",
            false,
        )
        .expect("restore source revision as draft");
    assert_eq!(restored.base_revision_id, "revision-1");
    assert!(restored.selected_items.is_empty());
    assert!(restored.pinned_item_ids.is_empty());
    assert_ne!(restored.draft_id, "revision-1");
}

#[test]
fn journal_recovery_rejects_tampered_or_duplicate_retained_items() {
    let plane = ControlPlane::synthetic();
    let journal = plane.export_journal().expect("journal");
    let mut tampered: Value = serde_json::from_slice(&journal).expect("journal JSON");
    let first = tampered
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .and_then(|items| items.first_mut())
        .and_then(Value::as_array_mut)
        .and_then(|entry| entry.get_mut(1))
        .and_then(Value::as_object_mut)
        .expect("item record");
    first.insert(
        "content".to_owned(),
        serde_json::json!([116, 97, 109, 112, 101, 114, 101, 100]),
    );
    let tampered = serde_json::to_vec(&tampered).expect("tampered journal");
    assert_eq!(
        ControlPlane::recover_journal(&tampered)
            .expect_err("content digest mismatch must be rejected")
            .code,
        "journal_invalid"
    );

    let mut duplicate: Value = serde_json::from_slice(&journal).expect("journal JSON");
    let items = duplicate
        .get_mut("items")
        .and_then(Value::as_array_mut)
        .expect("items");
    let first = items.first().cloned().expect("first item");
    items.push(first);
    let duplicate = serde_json::to_vec(&duplicate).expect("duplicate journal");
    assert_eq!(
        ControlPlane::recover_journal(&duplicate)
            .expect_err("duplicate item references must be rejected")
            .code,
        "journal_invalid"
    );

    let mut prepared_plane = ControlPlane::synthetic();
    let created = draft(&mut prepared_plane);
    let pause = command(
        &prepared_plane,
        "pause",
        "pause-manifest-tamper",
        prepared_plane.state().control_version,
    );
    prepared_plane.pause(pause).expect("pause");
    let preview = prepared_plane
        .create_preview(
            scope(&prepared_plane),
            &created.draft_id,
            created.version,
            true,
            prepared_plane.state().control_version,
            true,
        )
        .expect("prepared preview");
    assert!(preview.applicable);
    let mut tampered_manifest: Value =
        serde_json::from_slice(&prepared_plane.export_journal().expect("prepared journal"))
            .expect("prepared journal JSON");
    let preview_record = tampered_manifest
        .get_mut("plane")
        .and_then(Value::as_object_mut)
        .and_then(|plane| plane.get_mut("previews"))
        .and_then(Value::as_object_mut)
        .and_then(|previews| previews.get_mut(&preview.preview_id))
        .and_then(Value::as_object_mut)
        .and_then(|record| record.get_mut("material"))
        .and_then(Value::as_object_mut)
        .expect("prepared material");
    preview_record.insert("manifest_sha256".to_owned(), Value::String("0".repeat(64)));
    let tampered_manifest = serde_json::to_vec(&tampered_manifest).expect("tampered manifest");
    assert_eq!(
        ControlPlane::recover_journal(&tampered_manifest)
            .expect_err("manifest digest mismatch must be rejected")
            .code,
        "journal_invalid"
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
    assert!(plane.prepared_input().is_none());
    assert!(
        plane
            .events()
            .iter()
            .filter(|event| event.event_type == "input.submitted")
            .count()
            == 1
    );
    assert!(plane.validate_plan_epoch(plane.state().plan_epoch).is_ok());
}

#[test]
fn preview_reports_unknown_total_budget_until_operator_acknowledges_it() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let pause = command(
        &plane,
        "pause",
        "pause-budget-ack",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");

    let unacknowledged = plane
        .create_preview(
            scope(&plane),
            &created.draft_id,
            created.version,
            true,
            plane.state().control_version,
            false,
        )
        .expect("unacknowledged preview");
    assert!(!unacknowledged.applicable);
    assert_eq!(unacknowledged.blockers, vec!["unknown_total_budget"]);
    assert_eq!(unacknowledged.budget_status, "bounded_unknown_total");
    assert!(!unacknowledged.unknown_total_risk_acknowledged);

    let acknowledged = plane
        .create_preview(
            scope(&plane),
            &created.draft_id,
            created.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("acknowledged preview");
    assert!(acknowledged.applicable);
    assert!(acknowledged.blockers.is_empty());
    assert_eq!(acknowledged.budget_status, "bounded_unknown_total");
    assert!(acknowledged.unknown_total_risk_acknowledged);
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
