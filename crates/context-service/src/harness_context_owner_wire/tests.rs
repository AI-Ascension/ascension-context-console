use std::collections::BTreeMap;

use super::validation::{bounded_json_bytes, decode_bounded_json, validate_json_shape};
use super::*;

const BINDING_DIGEST: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
const DEFINITION_DIGEST: &str = "404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f";
const SOURCE_DIGEST: &str = "a0a1a2a3a4a5a6a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebf";
const BOUNDARY_JSON: &str = r#"{"run_id":"run-test","episode_id":"episode-test","agent_id":"agent-test","state_id":"state-test","generation":42,"observation_sha256":"202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f","catalog_sha256":"404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f","adapter_revision":"adapter.v1","model_revision":"model.v2","configuration_sha256":"606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f","output_schema_sha256":"808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f","controller_epoch":3,"gate_epoch":5,"control_version":9}"#;

fn boundary() -> ContextBoundary {
    ContextBoundary {
        run_id: "run-test".to_owned(),
        episode_id: "episode-test".to_owned(),
        agent_id: "agent-test".to_owned(),
        state_id: "state-test".to_owned(),
        generation: 42,
        observation_sha256: "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f"
            .to_owned(),
        catalog_sha256: DEFINITION_DIGEST.to_owned(),
        adapter_revision: "adapter.v1".to_owned(),
        model_revision: "model.v2".to_owned(),
        configuration_sha256: "606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f"
            .to_owned(),
        output_schema_sha256: "808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f"
            .to_owned(),
        controller_epoch: 3,
        gate_epoch: 5,
        control_version: 9,
    }
}

fn binding() -> ContextOwnerBinding {
    ContextOwnerBinding {
        schema_version: CONTEXT_OWNER_BINDING_SCHEMA_V1.to_owned(),
        owner_id: "owner-test".to_owned(),
        owner_version: "owner.v1".to_owned(),
        invocation_id: "invocation-test".to_owned(),
        binding_id: "binding-test".to_owned(),
        binding_version: 1,
        binding_digest: BINDING_DIGEST.to_owned(),
        context_ref: "context:test".to_owned(),
        instance_id: "instance-test".to_owned(),
        node_kind: "context-owner".to_owned(),
        state: ContextBindingState::Available,
        workflow_run_id: "run-test".to_owned(),
        definition_digest: DEFINITION_DIGEST.to_owned(),
        graph_id: "graph-test".to_owned(),
        node_id: "node-test".to_owned(),
        node_execution_id: "execution-test".to_owned(),
        boundary: boundary(),
        lease_epoch: 1,
        snapshot_id: "snapshot-test".to_owned(),
        approved_revision_id: "revision-4".to_owned(),
        plan_epoch: 7,
        grants: ContextBindingGrants {
            metadata_read: true,
            content_read: false,
            edit: false,
            control: false,
        },
        continuity: ContextBindingContinuity {
            survives_controller_restart: false,
            receipt_recovery: true,
            provider_session_continuity: false,
        },
    }
}

fn identity() -> OwnerIdentityCorrelationV2 {
    OwnerIdentityCorrelationV2 {
        console: ConsoleOwnerIdentityV2 {
            issuer: "https://identity.example/tenant/one".to_owned(),
            subject: "user-1".to_owned(),
            audience: "https://console.example/api".to_owned(),
            credential_id: "credential/provider/one".to_owned(),
            grant_id: "grant-1".to_owned(),
            grant_generation: 3,
            grant_expires_at: 2_000_000_000,
        },
        console_scope: ConsoleOwnerScopeV2 {
            project_id: "project-test".to_owned(),
            run_id: "run-test".to_owned(),
            episode_id: "episode-test".to_owned(),
            agent_id: "agent-test".to_owned(),
        },
        harness: HarnessActorIdentityV2 {
            actor_subject: "user-1".to_owned(),
            owner_id: "owner-test".to_owned(),
            workflow_run_id: "run-test".to_owned(),
            credential_reference_id: "broker://harness/user-1".to_owned(),
            credential_expires_at: 2_000_000_000,
        },
    }
}

fn patch_request() -> HarnessContextOwnerDraftPatchRequest {
    HarnessContextOwnerDraftPatchRequest {
        schema_version: CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1.to_owned(),
        request_id: "request-patch-1".to_owned(),
        draft_id: "draft-1".to_owned(),
        expected_version: 7,
        expected_boundary: boundary(),
        operations: vec![HarnessContextOwnerDraftOperation::PutNote {
            note_id: "note-1".to_owned(),
            text: "keep evidence".to_owned(),
        }],
    }
}

fn patch_invocation(request: HarnessContextOwnerDraftPatchRequest) -> ContextOwnerInvocationV2 {
    ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: identity(),
        expected_binding: Some(binding()),
        operation: ContextOwnerOperationV2::PatchDraft {
            workflow_run_id: "run-test".to_owned(),
            draft_id: "draft-1".to_owned(),
            request,
        },
    }
}

fn mutation_receipt(
    request: &HarnessContextOwnerDraftPatchRequest,
) -> HarnessContextOwnerMutationReceipt {
    let binding = binding();
    let draft = ContextDraft {
        schema: "ascension.context-control.draft.v1".to_owned(),
        draft_id: request.draft_id.clone(),
        version: request.expected_version + 1,
        base_revision_id: binding.approved_revision_id.clone(),
        selected_items: Vec::new(),
        pinned_item_ids: Vec::new(),
        notes: Vec::new(),
        objective: None,
        author_ref: "user-1".to_owned(),
    };
    HarnessContextOwnerMutationReceipt {
        schema_version: CONTEXT_OWNER_MUTATION_RECEIPT_SCHEMA_V1.to_owned(),
        owner_id: binding.owner_id.clone(),
        workflow_run_id: binding.workflow_run_id.clone(),
        actor_subject: "user-1".to_owned(),
        binding_id: binding.binding_id.clone(),
        binding_digest: binding.binding_digest.clone(),
        invocation_id: binding.invocation_id.clone(),
        boundary: binding.boundary.clone(),
        operation: "patch_draft".to_owned(),
        request_id: request.request_id.clone(),
        payload_digest: request.payload_digest().unwrap(),
        result: HarnessContextOwnerMutationResult::Draft(HarnessContextOwnerDraftEnvelope {
            schema_version: CONTEXT_OWNER_DRAFT_SCHEMA_V1.to_owned(),
            actor_subject: "user-1".to_owned(),
            binding,
            created_at: 100,
            updated_at: 100,
            retention_expires_at: None,
            draft,
        }),
        created_at: 100,
    }
}

#[test]
fn harness_mutation_request_bytes_and_digests_match_goldens() {
    let create = HarnessContextOwnerDraftCreateRequest {
        schema_version: CONTEXT_OWNER_DRAFT_REQUEST_SCHEMA_V1.to_owned(),
        request_id: "request-create-1".to_owned(),
        draft_id: "draft-1".to_owned(),
        base_revision_id: "revision-4".to_owned(),
        expected_boundary: boundary(),
    };
    let create_json = [
        r#"{"schema_version":"ascension.harness.context-owner-draft-request.v1","request_id":"request-create-1","draft_id":"draft-1","base_revision_id":"revision-4","expected_boundary":"#,
        BOUNDARY_JSON,
        "}",
    ]
    .concat();
    assert_eq!(serde_json::to_vec(&create).unwrap(), create_json.as_bytes());
    assert_eq!(
        create.payload_digest().unwrap(),
        "cc775350b5041413782c1bb43ce9cc35e0dd148fe8e6fce85b4a5057f56ba0df"
    );

    let patch = patch_request();
    let patch_json = [
        r#"{"schema_version":"ascension.harness.context-owner-draft-patch.v1","request_id":"request-patch-1","draft_id":"draft-1","expected_version":7,"expected_boundary":"#,
        BOUNDARY_JSON,
        r#","operations":[{"operation":"put_note","note_id":"note-1","text":"keep evidence"}]}"#,
    ]
    .concat();
    assert_eq!(serde_json::to_vec(&patch).unwrap(), patch_json.as_bytes());
    assert_eq!(
        patch.payload_digest().unwrap(),
        "a716df9fefda7415372d3cd5a4a8a814f2fee6bf7b4396623648a660071fa872"
    );

    let preview = HarnessContextOwnerPreviewRequest {
        schema_version: CONTEXT_OWNER_PREVIEW_REQUEST_SCHEMA_V1.to_owned(),
        request_id: "request-preview-1".to_owned(),
        draft_id: "draft-1".to_owned(),
        expected_version: 7,
        expected_boundary: boundary(),
    };
    let preview_json = [
        r#"{"schema_version":"ascension.harness.context-owner-preview-request.v1","request_id":"request-preview-1","draft_id":"draft-1","expected_version":7,"expected_boundary":"#,
        BOUNDARY_JSON,
        "}",
    ]
    .concat();
    assert_eq!(
        serde_json::to_vec(&preview).unwrap(),
        preview_json.as_bytes()
    );
    assert_eq!(
        preview.payload_digest().unwrap(),
        "14f207b00c84f21d5c3ac538fc185520e2b8dcca5d4018cb9e0af0cd4e194d1e"
    );
}

#[test]
fn patch_route_and_exact_lookup_keep_console_identity_out_of_harness_body() {
    let patch = patch_request();
    let invocation = patch_invocation(patch.clone());
    let endpoint = invocation.endpoint().unwrap();
    assert_eq!(endpoint.method.as_str(), "PATCH");
    assert_eq!(
        endpoint.path,
        "/v1/workflow-runs/run-test/context-owner-drafts/draft-1"
    );

    let body = invocation.harness_body().unwrap().unwrap();
    let patch_json = [
        r#"{"schema_version":"ascension.harness.context-owner-draft-patch.v1","request_id":"request-patch-1","draft_id":"draft-1","expected_version":7,"expected_boundary":"#,
        BOUNDARY_JSON,
        r#","operations":[{"operation":"put_note","note_id":"note-1","text":"keep evidence"}]}"#,
    ]
    .concat();
    assert_eq!(body, patch_json.as_bytes());
    let body_text = std::str::from_utf8(&body).unwrap();
    assert!(!body_text.contains("identity.example"));
    assert!(!body_text.contains("project-test"));

    let lookup = HarnessContextOwnerMutationLookupRequest {
        schema_version: CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1.to_owned(),
        request: HarnessContextOwnerMutationRequest::PatchDraft(patch.clone()),
    };
    let lookup_json = [
        r#"{"schema_version":"ascension.harness.context-owner-mutation-lookup.v1","request":{"kind":"patch_draft","request":"#,
        patch_json.as_str(),
        "}}",
    ]
    .concat();
    assert_eq!(serde_json::to_vec(&lookup).unwrap(), lookup_json.as_bytes());
    assert_eq!(
        lookup.request.payload_digest().unwrap(),
        patch.payload_digest().unwrap()
    );

    let mutation = HarnessContextOwnerMutationRequest::PatchDraft(patch.clone());
    let receipt = mutation_receipt(&patch);
    receipt.validate_for(&mutation, &binding()).unwrap();
    let response_bytes = serde_json::to_vec(&receipt).unwrap();
    assert!(matches!(
        invocation.decode_response(&response_bytes).unwrap(),
        HarnessResponseV1::MutationReceipt(_)
    ));

    let mut other_actor = receipt;
    other_actor.actor_subject = "user-2".to_owned();
    if let HarnessContextOwnerMutationResult::Draft(envelope) = &mut other_actor.result {
        envelope.actor_subject = "user-2".to_owned();
    }
    assert!(
        invocation
            .decode_response(&serde_json::to_vec(&other_actor).unwrap())
            .is_err()
    );

    let draft_envelope = match mutation_receipt(&patch).result {
        HarnessContextOwnerMutationResult::Draft(envelope) => envelope,
        HarnessContextOwnerMutationResult::Preview(_) => unreachable!(),
    };
    let mut get_invocation = ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: identity(),
        expected_binding: Some(binding()),
        operation: ContextOwnerOperationV2::GetDraft {
            workflow_run_id: "run-test".to_owned(),
            draft_id: "draft-1".to_owned(),
        },
    };
    let draft_bytes = serde_json::to_vec(&draft_envelope).unwrap();
    assert!(matches!(
        get_invocation.decode_response(&draft_bytes).unwrap(),
        HarnessResponseV1::Draft(_)
    ));
    get_invocation.operation = ContextOwnerOperationV2::GetDraft {
        workflow_run_id: "run-test".to_owned(),
        draft_id: "draft-other".to_owned(),
    };
    assert!(get_invocation.decode_response(&draft_bytes).is_err());
}

#[test]
fn returned_owner_views_must_match_the_full_console_run_scope() {
    let mut invocation = patch_invocation(patch_request());
    invocation.identity.console_scope.episode_id = "episode-other".to_owned();
    assert!(matches!(
        invocation.validate(),
        Err(OwnerWireError::CorrelationMismatch(
            "console_scope_boundary"
        ))
    ));
}

#[test]
fn source_status_uses_the_actual_get_route() {
    let invocation = ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: identity(),
        expected_binding: None,
        operation: ContextOwnerOperationV2::CurrentSourceStatus {
            workflow_run_id: "run-test".to_owned(),
        },
    };
    let endpoint = invocation.endpoint().unwrap();
    assert_eq!(endpoint.method.as_str(), "GET");
    assert_eq!(
        endpoint.path,
        "/v1/workflow-runs/run-test/context-owner-source-status"
    );
    assert!(invocation.harness_body().unwrap().is_none());
}

#[test]
fn endpoint_method_values_are_closed_http_verbs() {
    for (method, expected) in [
        (HarnessHttpMethod::Get, "GET"),
        (HarnessHttpMethod::Post, "POST"),
        (HarnessHttpMethod::Patch, "PATCH"),
        (HarnessHttpMethod::Put, "PUT"),
    ] {
        assert_eq!(method.as_str(), expected);
        assert_eq!(
            serde_json::to_string(&method).unwrap(),
            format!("\"{expected}\"")
        );
    }
}

#[test]
fn control_receipt_is_bound_to_the_exact_harness_command_and_boundary() {
    let binding = binding();
    let command = ContextControlCommand::Pause {
        idempotency_key: "control-test".to_owned(),
        expected_control_version: 9,
    };
    assert_eq!(
        serde_json::to_vec(&command).unwrap(),
        br#"{"pause":{"idempotency_key":"control-test","expected_control_version":9}}"#
    );
    command.validate(&binding).unwrap();

    let mut resulting_boundary = boundary();
    resulting_boundary.control_version = 10;
    resulting_boundary.gate_epoch = 6;
    let receipt = ContextControlReceipt {
        schema_version: CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2.to_owned(),
        owner_id: binding.owner_id.clone(),
        invocation_id: binding.invocation_id.clone(),
        binding_id: binding.binding_id.clone(),
        binding_digest: binding.binding_digest.clone(),
        command: ContextControlCommandKind::Pause,
        command_id: "command-test".to_owned(),
        idempotency_key: command.idempotency_key().to_owned(),
        effect: "pause_requested".to_owned(),
        control_version: 10,
        plan_epoch: binding.plan_epoch,
        controller_epoch: resulting_boundary.controller_epoch,
        gate_epoch: resulting_boundary.gate_epoch,
        boundary: resulting_boundary,
        revision_id: None,
        preview_manifest_digest: None,
        approved_manifest_digest: None,
    };
    receipt.validate_for(&binding, &command).unwrap();
    let changed_command = ContextControlCommand::Pause {
        idempotency_key: "other-control".to_owned(),
        expected_control_version: 9,
    };
    assert!(receipt.validate_for(&binding, &changed_command).is_err());
}

fn adoption_request() -> ContextSourceAdoptionRequest {
    ContextSourceAdoptionRequest {
        schema_version: CONTEXT_SOURCE_ADOPTION_SCHEMA_V1.to_owned(),
        idempotency_key: "adopt-source-1".to_owned(),
        expected_control_version: boundary().control_version,
        expected_revision_id: "revision-4".to_owned(),
        expected_boundary: boundary(),
    }
}

fn adoption_receipt(
    binding: &ContextOwnerBinding,
    request: &ContextSourceAdoptionRequest,
) -> ContextControlReceipt {
    let mut resulting_boundary = request.expected_boundary.clone();
    resulting_boundary.control_version += 1;
    let plan_epoch = binding.plan_epoch + 1;
    ContextControlReceipt {
        schema_version: CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2.to_owned(),
        owner_id: binding.owner_id.clone(),
        invocation_id: binding.invocation_id.clone(),
        binding_id: binding.binding_id.clone(),
        binding_digest: binding.binding_digest.clone(),
        command: ContextControlCommandKind::Commit,
        command_id: "command-adopt-1".to_owned(),
        idempotency_key: request.idempotency_key.clone(),
        effect: "revision_committed".to_owned(),
        control_version: resulting_boundary.control_version,
        plan_epoch,
        controller_epoch: resulting_boundary.controller_epoch,
        gate_epoch: resulting_boundary.gate_epoch,
        boundary: resulting_boundary,
        revision_id: Some(format!("revision-{plan_epoch}")),
        preview_manifest_digest: Some(SOURCE_DIGEST.to_owned()),
        approved_manifest_digest: Some(SOURCE_DIGEST.to_owned()),
    }
}

#[test]
fn adoption_receipt_checks_full_transition_then_refuses_without_source_witness() {
    let binding = binding();
    let request = adoption_request();
    let receipt = adoption_receipt(&binding, &request);
    assert_eq!(
        receipt.validate_adoption_for(&binding, &request),
        Err(OwnerWireError::CorrelationMismatch(
            "adoption_source_witness_required"
        ))
    );

    let mut wrong_actor_binding = binding.clone();
    wrong_actor_binding.invocation_id = "invocation-other".to_owned();
    assert!(matches!(
        receipt.validate_adoption_for(&wrong_actor_binding, &request),
        Err(OwnerWireError::CorrelationMismatch("adoption_receipt"))
    ));

    let mut wrong_request = request.clone();
    wrong_request.expected_boundary.episode_id = "episode-other".to_owned();
    assert!(matches!(
        receipt.validate_adoption_for(&binding, &wrong_request),
        Err(OwnerWireError::CorrelationMismatch("adoption_precondition"))
    ));

    let mut wrong_epoch = receipt.clone();
    wrong_epoch.plan_epoch += 1;
    assert!(matches!(
        wrong_epoch.validate_adoption_for(&binding, &request),
        Err(OwnerWireError::CorrelationMismatch("adoption_receipt"))
    ));

    let mut wrong_source_digest = receipt;
    wrong_source_digest.approved_manifest_digest = Some(DEFINITION_DIGEST.to_owned());
    assert!(matches!(
        wrong_source_digest.validate_adoption_for(&binding, &request),
        Err(OwnerWireError::CorrelationMismatch(
            "adoption_source_digest"
        ))
    ));
}

#[test]
fn bounded_json_shape_scan_rejects_duplicate_keys_at_every_object_level() {
    let duplicate_objects: [&[u8]; 4] = [
        br#"{"schema_version":"one","schema_version":"two"}"#,
        br#"{"actor_subject":"user-1","actor_subject":"user-2"}"#,
        br#"{"expected_boundary":{"control_version":9,"control_version":10}}"#,
        br#"{"document":{"items":{"item-a":0,"\u0069tem-a":1}}}"#,
    ];
    for duplicate in duplicate_objects {
        assert_eq!(
            validate_json_shape(duplicate),
            Err(OwnerWireError::JsonDecoding)
        );
    }
}

#[test]
fn bounded_json_decode_rejects_nested_unknown_fields_depth_and_body_overflow() {
    let create = HarnessContextOwnerDraftCreateRequest {
        schema_version: CONTEXT_OWNER_DRAFT_REQUEST_SCHEMA_V1.to_owned(),
        request_id: "request-create-1".to_owned(),
        draft_id: "draft-1".to_owned(),
        base_revision_id: "revision-4".to_owned(),
        expected_boundary: boundary(),
    };
    let mut json = String::from_utf8(serde_json::to_vec(&create).unwrap()).unwrap();
    json = json.replace(
        "\"expected_boundary\":{\"run_id\":",
        "\"expected_boundary\":{\"unknown_boundary_field\":true,\"run_id\":",
    );
    assert!(decode_bounded_json::<HarnessContextOwnerDraftCreateRequest>(json.as_bytes()).is_err());

    let depth = MAX_HARNESS_JSON_DEPTH + 1;
    let deeply_nested = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
    assert_eq!(
        validate_json_shape(deeply_nested.as_bytes()),
        Err(OwnerWireError::OutOfBounds("json_shape"))
    );

    let oversized = vec![b' '; MAX_HARNESS_JSON_BODY_BYTES + 1];
    assert_eq!(
        validate_json_shape(&oversized),
        Err(OwnerWireError::OutOfBounds("json_body"))
    );
}

#[test]
fn bounded_json_decode_accepts_the_full_context_byte_array_limit() {
    let upload = ContextSourceUpload {
        schema_version: CONTEXT_SOURCE_UPLOAD_SCHEMA_V1.to_owned(),
        document: ContextSourceDocument {
            draft: ContextDraft {
                schema: "ascension.context-control.draft.v1".to_owned(),
                draft_id: "draft-source-1".to_owned(),
                version: 1,
                base_revision_id: "revision-4".to_owned(),
                selected_items: Vec::new(),
                pinned_item_ids: Vec::new(),
                notes: Vec::new(),
                objective: None,
                author_ref: "user-1".to_owned(),
            },
            items: BTreeMap::from([(
                "item-source-1".to_owned(),
                ContextItem {
                    reference: ContextItemRef {
                        item_id: "item-source-1".to_owned(),
                        version: 1,
                        sha256: SOURCE_DIGEST.to_owned(),
                    },
                    kind: "note".to_owned(),
                    bytes: vec![0; MAX_HARNESS_CONTEXT_BYTES],
                    protected: false,
                    expires_at: 100,
                },
            )]),
        },
    };
    upload.validate().unwrap();
    let bytes = bounded_json_bytes(&upload).unwrap();
    assert!(bytes.len() <= MAX_HARNESS_JSON_BODY_BYTES);
    let decoded: ContextSourceUpload = decode_bounded_json(&bytes).unwrap();
    decoded.validate().unwrap();
    assert_eq!(
        decoded.document.items["item-source-1"].bytes.len(),
        MAX_HARNESS_CONTEXT_BYTES
    );
}

#[test]
fn publication_digest_and_receipt_keep_harness_correlation_contract() {
    let request = HarnessContextOwnerDraftPublicationRequest {
        schema_version: CONTEXT_OWNER_PUBLICATION_REQUEST_SCHEMA_V1.to_owned(),
        request_id: "request-test-0001".to_owned(),
        draft_id: "draft-test-0002".to_owned(),
        expected_draft_version: 7,
        expected_owner_state_version: 11,
        expected_base_revision_id: "revision-4".to_owned(),
        expected_binding_id: "binding-test".to_owned(),
        expected_binding_digest: BINDING_DIGEST.to_owned(),
        expected_boundary: boundary(),
    };
    let digest = "d723f9d3f5cea47713025ffaf09889201f22dc8305a8a69f2646f88cf4b1333c";
    assert_eq!(request.request_digest().unwrap(), digest);

    let receipt = HarnessContextOwnerDraftPublicationReceipt {
        schema_version: CONTEXT_OWNER_PUBLICATION_RECEIPT_SCHEMA_V1.to_owned(),
        owner_id: "owner-test".to_owned(),
        workflow_run_id: "run-test".to_owned(),
        actor_subject: "user-1".to_owned(),
        binding: binding(),
        boundary: boundary(),
        request_id: request.request_id.clone(),
        request_digest: digest.to_owned(),
        draft_id: request.draft_id.clone(),
        draft_version: request.expected_draft_version,
        base_revision_id: request.expected_base_revision_id.clone(),
        expected_owner_state_version: request.expected_owner_state_version,
        resulting_owner_state_version: 12,
        source_id: "source-test".to_owned(),
        source_version: 1,
        source_digest: SOURCE_DIGEST.to_owned(),
        published_at: 100,
        expires_at: 200,
    };
    receipt
        .validate_for(&request, "user-1", "run-test")
        .unwrap();
    let mut mismatched = receipt;
    mismatched.request_digest = SOURCE_DIGEST.to_owned();
    assert!(
        mismatched
            .validate_for(&request, "user-1", "run-test")
            .is_err()
    );
}

#[test]
fn initial_sqlite_grant_generation_zero_is_valid_only_with_live_expiry() {
    let mut value = identity();
    value.console.grant_generation = 0;
    value
        .validate()
        .expect("initial grant generation is exact row state");
    value.console.grant_expires_at = 0;
    assert!(value.validate().is_err());
}
