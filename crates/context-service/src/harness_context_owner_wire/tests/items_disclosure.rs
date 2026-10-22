use super::*;

fn invocation(include_content: Option<bool>, expected_binding: bool) -> ContextOwnerInvocationV2 {
    ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: identity(),
        expected_binding: expected_binding.then(binding),
        operation: ContextOwnerOperationV2::EligibleItems {
            workflow_run_id: "run-test".to_owned(),
            query: ContextOwnerItemsQueryV1 {
                draft_id: None,
                include_content,
            },
        },
    }
}

fn response(content: bool) -> serde_json::Value {
    let binding = binding();
    let mut item = serde_json::json!({
        "reference": { "item_id": "item-1", "version": 1, "sha256": SOURCE_DIGEST },
        "kind": "strategy", "byte_length": 3, "protected": false, "expires_at": 2_000_000_000,
        "source_id": "source-1", "source_version": 1, "source_digest": SOURCE_DIGEST,
    });
    if content {
        item["content"] = serde_json::json!([97, 98, 99]);
    }
    serde_json::json!({
        "schema_version": CONTEXT_OWNER_ITEMS_SCHEMA_V1,
        "owner_id": binding.owner_id, "workflow_run_id": binding.workflow_run_id,
        "binding_id": binding.binding_id, "binding_digest": binding.binding_digest,
        "boundary": binding.boundary, "items": [item],
    })
}

#[test]
fn metadata_queries_reject_content_for_both_default_and_explicit_selection() {
    let bytes = serde_json::to_vec(&response(true)).unwrap();
    for include_content in [None, Some(false)] {
        for expected_binding in [false, true] {
            let request = invocation(include_content, expected_binding);
            request.validate().unwrap();
            assert_eq!(
                request.decode_response(&bytes),
                Err(OwnerWireError::CorrelationMismatch(
                    "items_content_selection"
                )),
                "metadata-only response cannot disclose owner content: {include_content:?}, binding={expected_binding}",
            );
        }
    }
}

#[test]
fn metadata_queries_reject_content_anywhere_in_a_mixed_item_list() {
    let mut value = response(false);
    let mut retained = response(true)["items"][0].clone();
    retained["reference"]["item_id"] = serde_json::json!("item-2");
    value["items"].as_array_mut().unwrap().push(retained);
    assert_eq!(
        invocation(Some(false), true).decode_response(&serde_json::to_vec(&value).unwrap()),
        Err(OwnerWireError::CorrelationMismatch(
            "items_content_selection"
        )),
    );
}

#[test]
fn metadata_views_and_explicit_content_views_remain_accepted() {
    let metadata = serde_json::to_vec(&response(false)).unwrap();
    for include_content in [None, Some(false), Some(true)] {
        for expected_binding in [false, true] {
            let HarnessResponseV1::Items(view) = invocation(include_content, expected_binding)
                .decode_response(&metadata)
                .unwrap()
            else {
                panic!("eligible item response");
            };
            assert!(view.items.iter().all(|item| item.content.is_none()));
        }
    }
    let content = serde_json::to_vec(&response(true)).unwrap();
    for expected_binding in [false, true] {
        let HarnessResponseV1::Items(view) = invocation(Some(true), expected_binding)
            .decode_response(&content)
            .unwrap()
        else {
            panic!("eligible item response");
        };
        assert_eq!(view.items[0].content.as_deref(), Some(b"abc".as_slice()));
    }
}
