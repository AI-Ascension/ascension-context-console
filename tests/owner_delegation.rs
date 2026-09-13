// SPDX-License-Identifier: MIT

use context_service::{
    HarnessOwner, HarnessOwnerComposition, MemoryRoute, MemoryRouteError, MemoryScope, OwnerError,
    OwnerGrant, OwnerGrantBook, OwnerGrantClass, OwnerOperation, OwnerOutcome, OwnerReceipt,
    OwnerReceiptLookup, OwnerReply, OwnerRequestContext, OwnerScope, ProviderSessionRoute,
    SessionApiError, SessionScopeView,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::MutexGuard;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Eq, PartialEq)]
struct SeenCall {
    operation: OwnerOperation,
    scope: OwnerScope,
    reference: Option<String>,
    payload: Value,
}

#[derive(Default)]
struct RecordingOwner {
    calls: Mutex<Vec<SeenCall>>,
    lookups: Mutex<Vec<OwnerReceiptLookup>>,
    receipts: Mutex<BTreeMap<String, OwnerReply>>,
    lose_next: Mutex<Option<String>>,
    unsupported: Mutex<bool>,
    unknown: Mutex<bool>,
    forbidden_response: Mutex<bool>,
    lookup_error: Mutex<Option<OwnerError>>,
    lookup_mismatch: Mutex<bool>,
}

impl RecordingOwner {
    fn accepted(
        &self,
        operation: OwnerOperation,
        scope: &OwnerScope,
        payload: &Value,
    ) -> Result<OwnerReply, OwnerError> {
        self.accepted_with_receipt(operation, scope, payload, None)
    }

    fn accepted_with_receipt(
        &self,
        operation: OwnerOperation,
        scope: &OwnerScope,
        payload: &Value,
        receipt_id_override: Option<&str>,
    ) -> Result<OwnerReply, OwnerError> {
        let receipt_id = receipt_id_override
            .map(ToOwned::to_owned)
            .or_else(|| {
                payload
                    .get("idempotency_key")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| format!("receipt-{}", lock(&self.calls).len()));
        let operation_id = format!("operation-{}", lock(&self.calls).len());
        let mut value = json!({
            "schema": "owner.fixture.v1",
            "operation": operation.as_str(),
            "inference_calls": 0,
            "native_calls": 0,
            "game_effects": 0,
        });
        if operation == OwnerOperation::SessionCandidate {
            value["binding_id"] = Value::String("binding-owner-1".to_owned());
            value["operation_id"] = Value::String(operation_id.clone());
        } else if operation == OwnerOperation::SessionCompaction {
            value["operation_id"] = Value::String(operation_id.clone());
        }
        if *lock(&self.forbidden_response) {
            value["private_content"] = Value::String("must-not-cross-boundary".to_owned());
        }
        let receipt = OwnerReceipt::new(
            receipt_id.clone(),
            operation_id,
            operation,
            "harness",
            7,
            "synthetic_owner",
            OwnerOutcome::Accepted,
            false,
        )
        .map_err(|_| OwnerError::MalformedResponse)?;
        let reply = if *self
            .forbidden_response
            .lock()
            .map_err(|_| OwnerError::MalformedResponse)?
        {
            // Deliberately bypass the helper's validation so the route's consumer-side response
            // fence is exercised.
            OwnerReply {
                receipt,
                value: Some(value),
            }
        } else {
            OwnerReply::new(receipt, Some(value)).map_err(|_| OwnerError::MalformedResponse)?
        };
        lock(&self.receipts).insert(receipt_id, reply.clone());
        let _ = scope;
        Ok(reply)
    }

    fn call_count(&self) -> usize {
        lock(&self.calls).len()
    }

    fn lookup_count(&self) -> usize {
        lock(&self.lookups).len()
    }
}

impl HarnessOwner for RecordingOwner {
    fn call(&self, request: context_service::OwnerCall) -> Result<OwnerReply, OwnerError> {
        lock(&self.calls).push(SeenCall {
            operation: request.operation,
            scope: request.scope.clone(),
            reference: request.reference.clone(),
            payload: request.payload.clone(),
        });
        if let Some(receipt_id) = lock(&self.lose_next).take() {
            let reply = self.accepted_with_receipt(
                request.operation,
                &request.scope,
                &request.payload,
                Some(&receipt_id),
            )?;
            lock(&self.receipts).insert(receipt_id.clone(), reply);
            return Err(OwnerError::LostReply { receipt_id });
        }
        if *lock(&self.unsupported) {
            return OwnerReply::unsupported(
                "unsupported-receipt",
                "unsupported",
                request.operation,
                "harness",
                7,
                "owner_unsupported",
            )
            .map_err(|_| OwnerError::MalformedResponse);
        }
        if *lock(&self.unknown) {
            return OwnerReply::unknown(
                "unknown-receipt",
                "unknown-operation",
                request.operation,
                "harness",
                7,
                "owner_unknown",
            )
            .map_err(|_| OwnerError::MalformedResponse);
        }
        self.accepted(request.operation, &request.scope, &request.payload)
    }

    fn lookup_receipt(&self, request: OwnerReceiptLookup) -> Result<OwnerReply, OwnerError> {
        lock(&self.lookups).push(request.clone());
        if let Some(error) = lock(&self.lookup_error).take() {
            return Err(error);
        }
        if *lock(&self.lookup_mismatch) {
            return OwnerReply::unknown(
                "different-receipt",
                "different-operation",
                request.operation,
                "harness",
                7,
                "mismatched_receipt",
            )
            .map_err(|_| OwnerError::MalformedResponse);
        }
        lock(&self.receipts)
            .get(&request.receipt_id)
            .cloned()
            .ok_or(OwnerError::UnknownReceipt)
    }
}

fn memory_scope() -> MemoryScope {
    MemoryScope {
        project_id: "project-owner".to_owned(),
        run_id: "run-owner".to_owned(),
        episode_id: "episode-owner".to_owned(),
        agent_id: "agent-owner".to_owned(),
    }
}

fn owner_scope() -> OwnerScope {
    OwnerScope {
        project_id: "project-owner".to_owned(),
        run_id: "run-owner".to_owned(),
        episode_id: "episode-owner".to_owned(),
        agent_id: "agent-owner".to_owned(),
    }
}

fn session_scope() -> OwnerScope {
    OwnerScope {
        project_id: "project-fixture".to_owned(),
        run_id: "run-owner".to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    }
}

fn grant_book(scope: OwnerScope, expiry: u64) -> OwnerGrantBook {
    OwnerGrantBook::new(
        [
            OwnerGrant::new(
                "read-grant",
                OwnerGrantClass::ReadSearch,
                scope.clone(),
                expiry,
                "console.test",
                Some("https://console.test".to_owned()),
                None,
            )
            .ok(),
            OwnerGrant::new(
                "review-grant",
                OwnerGrantClass::GenerationReview,
                scope.clone(),
                expiry,
                "console.test",
                Some("https://console.test".to_owned()),
                Some("csrf-review".to_owned()),
            )
            .ok(),
            OwnerGrant::new(
                "control-grant",
                OwnerGrantClass::Control,
                scope,
                expiry,
                "console.test",
                Some("https://console.test".to_owned()),
                Some("csrf-control".to_owned()),
            )
            .ok(),
        ]
        .into_iter()
        .flatten(),
    )
}

fn context(principal: &str, csrf_token: Option<&str>, now: u64) -> OwnerRequestContext {
    OwnerRequestContext::new(
        principal,
        "console.test",
        Some("https://console.test".to_owned()),
        csrf_token.map(ToOwned::to_owned),
        now,
    )
}

#[allow(clippy::manual_unwrap_or_default)]
fn query(scope: &MemoryScope) -> Vec<u8> {
    match serde_json::to_vec(&json!({
        "schema": "ascension.context-memory.query.v1",
        "scope": scope,
        "branch_id": "branch-a",
        "query": "HP settled",
        "cutoff": 10,
        "corpus_generation": 1,
        "ranker_version": "lexical-v1",
        "limit": 8,
        "max_candidates": 64,
        "effect_class": "local_read_no_inference"
    })) {
        Ok(bytes) => bytes,
        Err(_) => Vec::new(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[test]
fn attached_memory_delegates_exact_query_and_keeps_lanes_independent() -> Result<(), String> {
    let owner = Arc::new(RecordingOwner::default());
    let route = MemoryRoute::attached(
        memory_scope(),
        HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100)),
    );
    let response = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        )
        .map_err(|error| format!("delegated search failed: {error:?}"))?;
    assert_eq!(response["outcome"], "accepted");
    assert_eq!(response["receipt"]["operation"], "memory.query");
    assert_eq!(owner.call_count(), 1);
    assert_eq!(
        lock(&owner.calls).first().map(|call| call.operation),
        Some(OwnerOperation::MemoryQuery)
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/generate",
            &context("read-grant", None, 10),
            br#"{"idempotency_key":"generate-1","proposal_id":"proposal-1"}"#,
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    assert_eq!(owner.call_count(), 1);
    let generated = route
        .handle_with_context(
            "POST",
            "/v3/memory/generate",
            &context("review-grant", Some("csrf-review"), 10),
            br#"{"idempotency_key":"generate-1","proposal_id":"proposal-1"}"#,
        )
        .map_err(|error| format!("delegated generation failed: {error:?}"))?;
    assert_eq!(generated["receipt"]["operation"], "memory.generation");
    assert_eq!(owner.call_count(), 2);
    Ok(())
}

#[test]
fn attached_memory_delegates_status_selection_and_review_lanes() -> Result<(), String> {
    let owner = Arc::new(RecordingOwner::default());
    let route = MemoryRoute::attached(
        memory_scope(),
        HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100)),
    );
    let status = route
        .handle_with_context(
            "GET",
            "/v3/memory/status",
            &context("read-grant", None, 10),
            &[],
        )
        .map_err(|error| format!("status failed: {error:?}"))?;
    assert_eq!(status["receipt"]["operation"], "memory.status");
    let review = route
        .handle_with_context(
            "POST",
            "/v3/memory/review",
            &context("review-grant", Some("csrf-review"), 10),
            br#"{"idempotency_key":"review-1","proposal_id":"proposal-1","decision":"admit"}"#,
        )
        .map_err(|error| format!("review failed: {error:?}"))?;
    assert_eq!(review["receipt"]["operation"], "memory.review");
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/selection",
            &context("review-grant", Some("csrf-review"), 10),
            br#"{"idempotency_key":"selection-1","selection_id":"selection-1"}"#,
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    let selection = route
        .handle_with_context(
            "POST",
            "/v3/memory/selection",
            &context("control-grant", Some("csrf-control"), 10),
            br#"{"idempotency_key":"selection-1","selection_id":"selection-1"}"#,
        )
        .map_err(|error| format!("selection failed: {error:?}"))?;
    assert_eq!(selection["receipt"]["operation"], "memory.selection");
    assert_eq!(owner.call_count(), 3);
    Ok(())
}

#[test]
fn attached_compatibility_handlers_require_explicit_security_context() {
    let owner = Arc::new(RecordingOwner::default());
    let composition = HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100));
    let memory = MemoryRoute::attached(memory_scope(), composition.clone());
    assert_eq!(
        memory.handle(
            "POST",
            "/v3/memory/generate",
            "review-grant",
            br#"{"idempotency_key":"generate-1","proposal_id":"proposal-1"}"#,
        ),
        Err(MemoryRouteError::PermissionDenied)
    );

    let mut sessions = ProviderSessionRoute::attached("session-principal", composition);
    assert_eq!(
        sessions.handle(
            "GET",
            "/v1/runs/run-owner/provider-sessions/capabilities",
            "read-grant",
            &[],
        ),
        Err(SessionApiError::Unauthorized)
    );
    assert_eq!(owner.call_count(), 0);
}

#[test]
fn attached_mutation_payloads_are_closed_and_typed_before_forwarding() {
    let owner = Arc::new(RecordingOwner::default());
    let composition = HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100));
    let memory = MemoryRoute::attached(memory_scope(), composition.clone());
    for body in [
        br#"{"idempotency_key":"generate-1","proposal_id":7}"#.as_slice(),
        br#"{"idempotency_key":"generate-2","proposal_id":"proposal-1","nested":{"url":"https://evil.test"}}"#.as_slice(),
        br#"{"idempotency_key":"review-1","proposal_id":"proposal-1","decision":"unknown"}"#.as_slice(),
        br#"{"idempotency_key":"selection-1","selection_id":"selection-1","nested":{"path":"/etc/passwd"}}"#.as_slice(),
    ] {
        let operation = if body.starts_with(b"{\"idempotency_key\":\"review") {
            "/v3/memory/review"
        } else if body.starts_with(b"{\"idempotency_key\":\"selection") {
            "/v3/memory/selection"
        } else {
            "/v3/memory/generate"
        };
        let request_context = if operation.ends_with("selection") {
            context("control-grant", Some("csrf-control"), 10)
        } else {
            context("review-grant", Some("csrf-review"), 10)
        };
        assert_eq!(
            memory.handle_with_context(
                "POST",
                operation,
                &request_context,
                body,
            ),
            Err(MemoryRouteError::InvalidRequest)
        );
    }

    let session_composition =
        HarnessOwnerComposition::new(owner.clone(), grant_book(session_scope(), 100));
    let mut sessions = ProviderSessionRoute::attached("session-principal", session_composition);
    sessions
        .register_attached_binding(
            "binding-owner-1",
            SessionScopeView {
                project_id: "project-fixture".to_owned(),
                run_id: "run-owner".to_owned(),
                episode_id: "episode-fixture".to_owned(),
                agent_id: "agent-fixture".to_owned(),
            },
        )
        .expect("binding");
    for body in [
        br#"{"idempotency_key":"candidate-1","expected_control_generation":"1","approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation"}"#.as_slice(),
        br#"{"idempotency_key":"candidate-2","expected_control_generation":1,"approved_policy_ref":"/etc/passwd","profile_ref":"profile-1","purpose":"evaluation"}"#.as_slice(),
        br#"{"idempotency_key":"candidate-3","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"https://evil.test","purpose":"evaluation"}"#.as_slice(),
        br#"{"idempotency_key":"candidate-4","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","nested":{"path":"/etc/passwd"}}"#.as_slice(),
    ] {
        assert_eq!(
            sessions.handle_with_context(
                "POST",
                "/v1/runs/run-owner/provider-sessions/candidates",
                &context("control-grant", Some("csrf-control"), 10),
                body,
            ),
            Err(SessionApiError::BadRequest)
        );
    }
    assert_eq!(owner.call_count(), 0);
}

#[test]
fn attached_memory_fails_closed_before_forwarding_for_security_and_scope() {
    let owner = Arc::new(RecordingOwner::default());
    let route = MemoryRoute::attached(
        memory_scope(),
        HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100)),
    );
    let forged_host = OwnerRequestContext::new(
        "read-grant",
        "evil.test",
        Some("https://console.test".to_owned()),
        None,
        10,
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &forged_host,
            &query(&memory_scope()),
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    let forged_origin = OwnerRequestContext::new(
        "read-grant",
        "console.test",
        Some("https://evil.test".to_owned()),
        None,
        10,
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &forged_origin,
            &query(&memory_scope()),
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/generate",
            &context("review-grant", Some("wrong-csrf"), 10),
            br#"{"idempotency_key":"generate-1","proposal_id":"proposal-1"}"#,
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 100),
            &query(&memory_scope()),
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    let foreign_scope = MemoryScope {
        run_id: "foreign-run".to_owned(),
        ..memory_scope()
    };
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&foreign_scope),
        ),
        Err(MemoryRouteError::InvalidRequest)
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/generate",
            &context("review-grant", Some("csrf-review"), 10),
            br#"{"idempotency_key":"bad-ref","path":"/tmp/provider"}"#,
        ),
        Err(MemoryRouteError::InvalidRequest)
    );
    assert_eq!(owner.call_count(), 0);
    assert!(route.revoke_grant("read-grant").is_ok());
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    assert_eq!(owner.call_count(), 0);
}

#[test]
fn lost_reply_looks_up_receipt_without_retrying_and_preserves_outcomes() -> Result<(), String> {
    let owner = Arc::new(RecordingOwner::default());
    *lock(&owner.lose_next) = Some("receipt-1".to_owned());
    let route = MemoryRoute::attached(
        memory_scope(),
        HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100)),
    );
    let response = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        )
        .map_err(|error| format!("receipt recovery failed: {error:?}"))?;
    assert_eq!(response["outcome"], "accepted");
    assert_eq!(owner.call_count(), 1);
    assert_eq!(owner.lookup_count(), 1);

    *lock(&owner.unsupported) = true;
    let response = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        )
        .map_err(|error| format!("unsupported receipt failed: {error:?}"))?;
    assert_eq!(response["outcome"], "unsupported");
    *lock(&owner.unsupported) = false;
    *lock(&owner.unknown) = true;
    let response = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        )
        .map_err(|error| format!("unknown receipt failed: {error:?}"))?;
    assert_eq!(response["outcome"], "unknown");
    Ok(())
}

#[test]
fn lost_reply_recovery_correlates_receipts_and_maps_lookup_failures() -> Result<(), String> {
    let owner = Arc::new(RecordingOwner::default());
    let route = MemoryRoute::attached(
        memory_scope(),
        HarnessOwnerComposition::new(owner.clone(), grant_book(owner_scope(), 100)),
    );

    *lock(&owner.lose_next) = Some("lost-mismatch".to_owned());
    *lock(&owner.lookup_mismatch) = true;
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        ),
        Err(MemoryRouteError::Unsupported)
    );

    *lock(&owner.lookup_mismatch) = false;
    *lock(&owner.lose_next) = Some("lost-unknown".to_owned());
    *lock(&owner.lookup_error) = Some(OwnerError::UnknownReceipt);
    let unknown = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        )
        .map_err(|error| format!("unknown lookup failed: {error:?}"))?;
    assert_eq!(unknown["outcome"], "unknown");

    *lock(&owner.lose_next) = Some("lost-unsupported".to_owned());
    *lock(&owner.lookup_error) = Some(OwnerError::Unsupported);
    let unsupported = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context("read-grant", None, 10),
            &query(&memory_scope()),
        )
        .map_err(|error| format!("unsupported lookup failed: {error:?}"))?;
    assert_eq!(unsupported["outcome"], "unsupported");

    let session_owner = Arc::new(RecordingOwner::default());
    let mut sessions = ProviderSessionRoute::attached(
        "session-principal",
        HarnessOwnerComposition::new(session_owner.clone(), grant_book(session_scope(), 100)),
    );
    sessions
        .register_attached_binding(
            "binding-owner-1",
            SessionScopeView {
                project_id: "project-fixture".to_owned(),
                run_id: "run-owner".to_owned(),
                episode_id: "episode-fixture".to_owned(),
                agent_id: "agent-fixture".to_owned(),
            },
        )
        .map_err(|error| format!("binding failed: {error:?}"))?;
    *lock(&session_owner.lose_next) = Some("session-mismatch".to_owned());
    *lock(&session_owner.lookup_mismatch) = true;
    assert_eq!(
        sessions.handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/history",
            &context("read-grant", None, 10),
            &[],
        ),
        Err(SessionApiError::MalformedPeer)
    );
    *lock(&session_owner.lookup_mismatch) = false;
    *lock(&session_owner.lose_next) = Some("session-unknown".to_owned());
    *lock(&session_owner.lookup_error) = Some(OwnerError::UnknownReceipt);
    let unknown = sessions
        .handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/history",
            &context("read-grant", None, 10),
            &[],
        )
        .map_err(|error| format!("session unknown lookup failed: {error:?}"))?;
    assert_eq!(unknown["outcome"], "unknown");

    *lock(&session_owner.lose_next) = Some("session-unsupported".to_owned());
    *lock(&session_owner.lookup_error) = Some(OwnerError::Unsupported);
    let unsupported = sessions
        .handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/history",
            &context("read-grant", None, 10),
            &[],
        )
        .map_err(|error| format!("session unsupported lookup failed: {error:?}"))?;
    assert_eq!(unsupported["outcome"], "unsupported");
    Ok(())
}

#[test]
fn attached_reference_indexes_bound_restore_and_owner_registration() {
    let owner = Arc::new(RecordingOwner::default());
    let mut route = ProviderSessionRoute::attached(
        "session-principal",
        HarnessOwnerComposition::new(owner.clone(), grant_book(session_scope(), 100)),
    );
    let scope = SessionScopeView {
        project_id: "project-fixture".to_owned(),
        run_id: "run-owner".to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    };
    for index in 0..128 {
        route
            .register_attached_binding(format!("binding-{index}"), scope.clone())
            .expect("binding capacity");
    }
    assert_eq!(
        route.register_attached_binding("binding-overflow", scope.clone()),
        Err(SessionApiError::Capacity)
    );
    assert!(
        route
            .register_attached_binding("binding-0", scope.clone())
            .is_ok()
    );

    for index in 0..512 {
        route
            .register_attached_operation(format!("operation-{index}"), scope.clone())
            .expect("operation capacity");
    }
    assert_eq!(
        route.register_attached_operation("operation-overflow", scope),
        Err(SessionApiError::Capacity)
    );

    let owner = Arc::new(RecordingOwner::default());
    let mut full_bindings = ProviderSessionRoute::attached(
        "session-principal",
        HarnessOwnerComposition::new(owner.clone(), grant_book(session_scope(), 100)),
    );
    let scope = SessionScopeView {
        project_id: "project-fixture".to_owned(),
        run_id: "run-owner".to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    };
    for index in 0..128 {
        full_bindings
            .register_attached_binding(format!("binding-{index}"), scope.clone())
            .expect("binding capacity");
    }
    assert_eq!(
        full_bindings.handle_with_context(
            "POST",
            "/v1/runs/run-owner/provider-sessions/candidates",
            &context("control-grant", Some("csrf-control"), 10),
            br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation"}"#,
        ),
        Err(SessionApiError::Capacity)
    );
    assert_eq!(owner.call_count(), 0);

    let owner = Arc::new(RecordingOwner::default());
    let mut full_operations = ProviderSessionRoute::attached(
        "session-principal",
        HarnessOwnerComposition::new(owner.clone(), grant_book(session_scope(), 100)),
    );
    full_operations
        .register_attached_binding("binding-owner-1", scope.clone())
        .expect("binding");
    for index in 0..512 {
        full_operations
            .register_attached_operation(format!("operation-{index}"), scope.clone())
            .expect("operation capacity");
    }
    assert_eq!(
        full_operations.handle_with_context(
            "POST",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/compaction-jobs",
            &context("control-grant", Some("csrf-control"), 10),
            br#"{"idempotency_key":"compact-1","expected_control_generation":1,"approved_compaction_plan_ref":"plan-1","spend_authorization_ref":"spend-1"}"#,
        ),
        Err(SessionApiError::Capacity)
    );
    assert_eq!(owner.call_count(), 0);
}

#[test]
fn attached_session_delegates_history_and_compaction_with_separate_control_grant()
-> Result<(), String> {
    let owner = Arc::new(RecordingOwner::default());
    let scope = session_scope();
    let mut route = ProviderSessionRoute::attached(
        "session-principal",
        HarnessOwnerComposition::new(owner.clone(), grant_book(scope, 100)),
    );
    route
        .register_attached_binding(
            "binding-owner-1",
            SessionScopeView {
                project_id: "project-fixture".to_owned(),
                run_id: "run-owner".to_owned(),
                episode_id: "episode-fixture".to_owned(),
                agent_id: "agent-fixture".to_owned(),
            },
        )
        .map_err(|error| format!("binding failed: {error:?}"))?;
    let capabilities = route
        .handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/capabilities",
            &context("read-grant", None, 10),
            &[],
        )
        .map_err(|error| format!("capabilities failed: {error:?}"))?;
    assert_eq!(
        capabilities["receipt"]["operation"],
        "provider_session.capabilities"
    );
    let status = route
        .handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/status",
            &context("read-grant", None, 10),
            &[],
        )
        .map_err(|error| format!("status failed: {error:?}"))?;
    assert_eq!(status["receipt"]["operation"], "provider_session.status");
    let history = route
        .handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/history",
            &context("read-grant", None, 10),
            &[],
        )
        .map_err(|error| format!("history failed: {error:?}"))?;
    assert_eq!(history["outcome"], "accepted");
    assert_eq!(history["receipt"]["operation"], "provider_session.history");
    assert_eq!(owner.call_count(), 3);
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/compaction-jobs",
            &context("read-grant", None, 10),
            br#"{"idempotency_key":"compact-1","expected_control_generation":1,"approved_compaction_plan_ref":"plan-1","spend_authorization_ref":"spend-1"}"#,
        ),
        Err(SessionApiError::Forbidden)
    );
    assert_eq!(owner.call_count(), 3);
    *lock(&owner.lose_next) = Some("receipt-4".to_owned());
    *lock(&owner.lookup_mismatch) = true;
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/compaction-jobs",
            &context("control-grant", Some("csrf-control"), 10),
            br#"{"idempotency_key":"compact-1","expected_control_generation":1,"approved_compaction_plan_ref":"plan-1","spend_authorization_ref":"spend-1"}"#,
        ),
        Err(SessionApiError::MalformedPeer)
    );
    assert_eq!(owner.call_count(), 4);
    assert_eq!(owner.lookup_count(), 1);
    *lock(&owner.lookup_mismatch) = false;
    *lock(&owner.lose_next) = Some("receipt-5".to_owned());
    let compact = route.handle_with_context(
        "POST",
        "/v1/runs/run-owner/provider-sessions/binding-owner-1/compaction-jobs",
        &context("control-grant", Some("csrf-control"), 10),
        br#"{"idempotency_key":"compact-1","expected_control_generation":1,"approved_compaction_plan_ref":"plan-1","spend_authorization_ref":"spend-1"}"#,
    )
    .map_err(|error| format!("compaction failed: {error:?}"))?;
    assert_eq!(
        compact["receipt"]["operation"],
        "provider_session.compaction"
    );
    assert_eq!(owner.call_count(), 5);
    assert_eq!(owner.lookup_count(), 2);
    assert_eq!(
        lock(&owner.calls)[4].payload["idempotency_key"],
        "compact-1"
    );
    Ok(())
}

#[test]
fn attached_session_rejects_foreign_references_and_forbidden_owner_content() {
    let owner = Arc::new(RecordingOwner::default());
    let scope = session_scope();
    let mut route = ProviderSessionRoute::attached(
        "session-principal",
        HarnessOwnerComposition::new(owner.clone(), grant_book(scope, 100)),
    );
    assert!(
        route
            .register_attached_binding(
                "binding-owner-1",
                SessionScopeView {
                    project_id: "project-fixture".to_owned(),
                    run_id: "run-owner".to_owned(),
                    episode_id: "episode-fixture".to_owned(),
                    agent_id: "agent-fixture".to_owned(),
                },
            )
            .is_ok()
    );
    assert_eq!(
        route.handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/foreign-binding/history",
            &context("read-grant", None, 10),
            &[],
        ),
        Err(SessionApiError::NotFound)
    );
    assert_eq!(owner.call_count(), 0);
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/reconnect",
            &context("control-grant", Some("csrf-control"), 10),
            br#"{"idempotency_key":"native-1","expected_control_generation":1,"expected_session_epoch":1,"native_method":"thread/start"}"#,
        ),
        Err(SessionApiError::BadRequest)
    );
    assert_eq!(owner.call_count(), 0);

    *lock(&owner.forbidden_response) = true;
    assert_eq!(
        route.handle_with_context(
            "GET",
            "/v1/runs/run-owner/provider-sessions/binding-owner-1/history",
            &context("read-grant", None, 10),
            &[],
        ),
        Err(SessionApiError::MalformedPeer)
    );
    assert_eq!(owner.call_count(), 1);
}

#[test]
fn unattached_modes_remain_explicit() -> Result<(), String> {
    let mut memory = MemoryRoute::new(memory_scope(), false);
    memory.grant_search("operator");
    assert!(!memory.is_attached());
    let response = memory
        .handle(
            "POST",
            "/v3/memory/search",
            "operator",
            &query(&memory_scope()),
        )
        .map_err(|error| format!("unavailable projection failed: {error:?}"))?;
    assert_eq!(response["coverage"], "projection_unavailable");

    let mut sessions = ProviderSessionRoute::disabled("operator");
    assert!(!sessions.is_attached());
    assert_eq!(
        sessions.handle(
            "GET",
            "/v1/runs/run-owner/provider-sessions",
            "operator",
            &[],
        ),
        Err(SessionApiError::Unsupported)
    );
    Ok(())
}

#[test]
fn owner_security_context_debug_redacts_csrf() {
    let context = context("control-grant", Some("csrf-secret"), 10);
    let debug = format!("{context:?}");
    assert!(!debug.contains("csrf-secret"));
    assert!(debug.contains("csrf_configured"));
}
