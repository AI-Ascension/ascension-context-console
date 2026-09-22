// SPDX-License-Identifier: MIT


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
fn restore_objective_requires_owner_authorization_at_the_control_boundary() {
    let mut plane = ControlPlane::synthetic();
    let created = draft(&mut plane);
    let objective = plane
        .apply_patch(
            patch(
                &plane,
                &created.draft_id,
                created.version,
                vec![ControlOperation::SetObjective {
                    text: "objective source".to_owned(),
                }],
            ),
            "objective-owner",
            true,
        )
        .expect("objective source");
    let pause = command(
        &plane,
        "pause",
        "objective-source-pause",
        plane.state().control_version,
    );
    plane.pause(pause).expect("pause");
    let preview = plane
        .create_preview(
            scope(&plane),
            &objective.draft_id,
            objective.version,
            true,
            plane.state().control_version,
            true,
        )
        .expect("preview");
    let preview_id = preview.preview_id.clone();
    let mut commit = command(
        &plane,
        "commit",
        "objective-source-commit",
        plane.state().control_version,
    );
    commit.expected_active_revision_id = Some("revision-1".to_owned());
    commit.preview_id = Some(preview_id.clone());
    commit.approved_manifest_sha256 = preview.prepared_manifest_sha256;
    let committed = plane.commit(commit).expect("commit");
    let mut resume = command(
        &plane,
        "resume",
        "objective-source-resume",
        plane.state().control_version,
    );
    resume.expected_active_revision_id = Some(committed.active_revision_id.clone());
    resume.expected_preview_id = Some(preview_id);
    plane.resume(resume).expect("resume");

    let fresh = draft(&mut plane);
    let error = plane
        .apply_patch(
            patch(
                &plane,
                &fresh.draft_id,
                fresh.version,
                vec![ControlOperation::RestoreConfiguration {
                    source_revision_id: committed.active_revision_id,
                }],
            ),
            "ordinary-owner",
            false,
        )
        .expect_err("restoring an objective requires owner authorization");
    assert_eq!(error.code, "objective_authorization_required");
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
