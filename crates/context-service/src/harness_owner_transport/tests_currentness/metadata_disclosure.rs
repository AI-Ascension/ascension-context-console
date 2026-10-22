use super::*;

#[test]
fn metadata_only_transport_refuses_an_owner_response_with_retained_content() {
    for include_content in [None, Some(false)] {
        let now = unix_now();
        let mut invocation = source_status_invocation();
        invocation.identity.console.grant_expires_at = now + 600;
        invocation.identity.harness.credential_expires_at = now + 600;
        invocation.operation = ContextOwnerOperationV2::EligibleItems {
            workflow_run_id: "run-1".to_owned(),
            query: ContextOwnerItemsQueryV1 {
                draft_id: None,
                include_content,
            },
        };
        let admission =
            TrustedInvocationAdmission::synthetic(&invocation, AdmissionUse::ReadOnly, now);
        let credential = credential(&invocation, &["workflow:read"], now + 600);
        let response = serde_json::to_vec(&serde_json::json!({
            "schema_version": CONTEXT_OWNER_ITEMS_SCHEMA_V1,
            "owner_id": "owner-1", "workflow_run_id": "run-1", "binding_id": "binding-1",
            "binding_digest": "a".repeat(64), "boundary": boundary(),
            "items": [{
                "reference": {"item_id": "item-1", "version": 1, "sha256": "b".repeat(64)},
                "kind": "strategy", "byte_length": 3, "protected": false, "expires_at": now + 600,
                "source_id": "source-1", "source_version": 1, "source_digest": "c".repeat(64),
                "content": [97, 98, 99],
            }],
        }))
        .unwrap();
        let (address, received) = serve_once(framed_json_response(&response), Duration::ZERO);
        let transport = transport_for(address, Duration::from_secs(1));
        let mut redeemer = TestRedeemer::default();
        let mut currentness = RevokeAt::new(usize::MAX, AdmissionUse::ReadOnly);
        let result = transport.send_read_once(
            &InvocationSendContext::borrowed(
                &invocation,
                &admission,
                &credential,
                Instant::now() + Duration::from_secs(2),
            ),
            &mut redeemer,
            &mut currentness,
        );
        let request = String::from_utf8(received.join().unwrap()).unwrap();
        assert!(request.starts_with("GET /v1/workflow-runs/run-1/context-owner-items"));
        assert_eq!(
            request.contains("include_content=false"),
            include_content == Some(false)
        );
        assert_eq!(result, Err(HarnessTransportError::InvalidOwnerResponse));
    }
}
