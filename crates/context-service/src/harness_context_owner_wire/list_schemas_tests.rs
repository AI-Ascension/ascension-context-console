use super::*;

// These labels come from the real producer's list_drafts_current and
// list_revisions_current responses at the owner-process-recovery lock. They
// deliberately share the singular envelope labels; operation selection owns
// the list shape. The process witness also checks populated production lists.
#[test]
fn draft_list_uses_the_producer_schema_and_keeps_the_closed_list_shape() {
    let bytes = br#"{"schema_version":"ascension.harness.context-owner-draft.v1","owner_id":"owner-test","workflow_run_id":"run-test","drafts":[]}"#;
    let view: HarnessContextOwnerDraftListView = decode_bounded_json(bytes).unwrap();
    view.validate().unwrap();
    let mut wrong_schema = view.clone();
    wrong_schema.schema_version = "ascension.harness.context-owner-drafts.v1".to_owned();
    assert!(wrong_schema.validate().is_err());
    let mut unknown = serde_json::to_value(&view).unwrap();
    unknown["authority"] = serde_json::json!(true);
    assert!(
        decode_bounded_json::<HarnessContextOwnerDraftListView>(
            &serde_json::to_vec(&unknown).unwrap()
        )
        .is_err()
    );
    assert!(decode_bounded_json::<HarnessContextOwnerDraftEnvelope>(bytes).is_err());
}

#[test]
fn revision_page_uses_the_producer_schema_and_keeps_the_closed_page_shape() {
    let bytes = br#"{"schema_version":"ascension.harness.context-owner-revision.v1","owner_id":"owner-test","workflow_run_id":"run-test","revisions":[]}"#;
    let view: HarnessContextOwnerRevisionPage = decode_bounded_json(bytes).unwrap();
    view.validate().unwrap();
    let mut wrong_schema = view.clone();
    wrong_schema.schema_version = "ascension.harness.context-owner-revisions.v1".to_owned();
    assert!(wrong_schema.validate().is_err());
    let mut unknown = serde_json::to_value(&view).unwrap();
    unknown["authority"] = serde_json::json!(true);
    assert!(
        decode_bounded_json::<HarnessContextOwnerRevisionPage>(
            &serde_json::to_vec(&unknown).unwrap()
        )
        .is_err()
    );
    assert!(decode_bounded_json::<HarnessContextOwnerRevisionEnvelope>(bytes).is_err());
}
