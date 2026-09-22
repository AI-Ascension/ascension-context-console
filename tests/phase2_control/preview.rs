// SPDX-License-Identifier: MIT


#[test]
fn restore_keeps_prior_intervention_lineage_visible_after_a_new_commit() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let edited = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::IncludeItem {
                    item: history_item(&plane),
                }],
            ),
            "operator-first",
            false,
        )
        .expect("first edit");
    let pause = command(
        &plane,
        "pause",
        "pause-lineage-first",
        plane.state().control_version,
    );
    plane.pause(pause).expect("first pause");
    let preview = plane
        .create_preview(
            scope(&plane),
            &edited.draft_id,
            edited.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("first preview");
    let mut commit = command(
        &plane,
        "commit",
        "commit-lineage-first",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some(plane.state().active_revision_id);
    commit.preview_id = Some(preview.preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    plane.commit(commit).expect("first commit");
    let first_revision = plane.state().active_revision_id.clone();
    let first_relation = plane
        .relations()
        .into_iter()
        .last()
        .expect("first intervention relation");
    let first_intervention = first_relation
        .intervention_id
        .clone()
        .expect("first intervention id");

    let mut resume = command(
        &plane,
        "resume",
        "resume-lineage-first",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(first_revision.clone());
    resume.expected_preview_id = Some(preview.preview_id);
    plane.resume(resume).expect("first resume");

    let restored_draft = draft(&mut plane);
    let restored = plane
        .apply_patch(
            patch(
                &plane,
                &restored_draft.draft_id,
                restored_draft.version,
                vec![ControlOperation::RestoreConfiguration {
                    source_revision_id: "revision-1".to_owned(),
                }],
            ),
            "operator-restore",
            false,
        )
        .expect("restore prior configuration");
    assert!(restored.selected_items.is_empty());

    let pause = command(
        &plane,
        "pause",
        "pause-lineage-restore",
        plane.state().control_version,
    );
    plane.pause(pause).expect("restore pause");
    let restored_preview = plane
        .create_preview(
            scope(&plane),
            &restored.draft_id,
            restored.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("restore preview");
    let mut restored_commit = command(
        &plane,
        "commit",
        "commit-lineage-restore",
        plane.state().control_version,
    );
    restored_commit.expected_active_revision_id = Some(first_revision.clone());
    restored_commit.preview_id = Some(restored_preview.preview_id.clone());
    restored_commit.approved_manifest_sha256 = restored_preview.prepared_manifest_sha256;
    plane
        .commit(restored_commit)
        .expect("commit restored configuration");

    let relations = plane.relations();
    assert_eq!(relations.len(), 2);
    assert_eq!(
        relations[0].intervention_id.as_deref(),
        Some(first_intervention.as_str())
    );
    assert_ne!(
        relations[1].intervention_id, relations[0].intervention_id,
        "restoring creates a distinct intervention while retaining the prior relation"
    );
    assert!(plane.revisions().iter().any(|revision| {
        revision.revision_id == first_revision
            && revision.intervention_id == first_intervention
            && revision.selected_items.len() == 1
    }));
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
