// SPDX-License-Identifier: MIT

use context_service::{ProviderSessionRoute, SessionApiError, SessionRouteMode};

#[test]
fn capabilities_are_scoped_and_explicitly_fixture_only() {
    let mut route = ProviderSessionRoute::fixture("operator");
    let value = route
        .handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions/capabilities",
            "operator",
            &[],
        )
        .expect("capabilities");
    assert_eq!(route.mode(), SessionRouteMode::FixtureOnly);
    assert_eq!(value["value"]["hardening"]["tools_enabled"], false);
    assert_eq!(value["value"]["raw_rpc"], false);
    assert_eq!(value["inference_calls"], 0);
    assert_eq!(
        route.handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions/capabilities",
            "other",
            &[],
        ),
        Err(SessionApiError::Unauthorized)
    );
    assert_eq!(
        route.handle(
            "GET",
            "/v1/runs/run-fixture/provider-session-events/capabilities",
            "operator",
            &[],
        ),
        Err(SessionApiError::NotFound)
    );
}

#[test]
fn candidate_is_an_async_metadata_operation_and_turn_route_is_absent() {
    let mut route = ProviderSessionRoute::fixture("operator");
    let candidate = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"executable_candidate"}"#,
        )
        .expect("candidate");
    assert_eq!(candidate["value"]["status"], "intent_persisted");
    assert_eq!(candidate["value"]["native_calls"], 0);
    assert_eq!(candidate["value"]["game_effects"], 0);
    let binding = candidate["value"]["binding_id"]
        .as_str()
        .expect("binding id");
    assert_eq!(
        route.handle(
            "POST",
            &format!("/v1/runs/run-fixture/provider-sessions/{binding}/turn/start"),
            "operator",
            b"{}",
        ),
        Err(SessionApiError::NotFound)
    );
    let list = route
        .handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions",
            "operator",
            &[],
        )
        .expect("list");
    assert_eq!(
        list["value"]["bindings"]
            .as_array()
            .expect("bindings")
            .len(),
        1
    );
}

#[test]
fn disabled_route_fails_closed_without_mutation() {
    let mut route = ProviderSessionRoute::disabled("operator");
    assert_eq!(
        route.handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions",
            "operator",
            &[],
        ),
        Err(SessionApiError::Unsupported)
    );
    assert_eq!(route.mode(), SessionRouteMode::Disabled);
}

#[test]
fn inspect_only_route_allows_reads_but_not_mutations() {
    let mut route = ProviderSessionRoute::inspect_only("operator");
    assert_eq!(route.mode(), SessionRouteMode::InspectOnly);
    route
        .handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions",
            "operator",
            &[],
        )
        .expect("list");
    assert_eq!(
        route.handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation"}"#,
        ),
        Err(SessionApiError::Unsupported)
    );
}

#[test]
fn commands_are_strictly_typed_and_idempotent() {
    let mut route = ProviderSessionRoute::fixture("operator");
    let request = br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation"}"#;
    let first = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            request,
        )
        .expect("candidate");
    let reordered = br#"{"purpose":"evaluation","profile_ref":"profile-1","approved_policy_ref":"policy-1","expected_control_generation":1,"idempotency_key":"candidate-1"}"#;
    let second = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            reordered,
        )
        .expect("duplicate candidate");
    assert_eq!(
        first["value"]["operation_id"],
        second["value"]["operation_id"]
    );
    let changed = br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-2","profile_ref":"profile-1","purpose":"evaluation"}"#;
    assert_eq!(
        route.handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            changed,
        ),
        Err(SessionApiError::Conflict)
    );
    assert_eq!(
        route.handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            br#"{"idempotency_key":"candidate-2","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","method":"thread/start"}"#,
        ),
        Err(SessionApiError::BadRequest)
    );
    assert_eq!(
        route.handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            b"",
        ),
        Err(SessionApiError::BadRequest)
    );
}

#[test]
fn maintenance_routes_are_metadata_only_and_scope_bound() {
    let mut route = ProviderSessionRoute::fixture("operator");
    let candidate = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"executable_candidate"}"#,
        )
        .expect("candidate");
    let binding = candidate["value"]["binding_id"].as_str().expect("binding");
    let reconnect = route
        .handle(
            "POST",
            &format!(
                "/v1/runs/run-fixture/provider-sessions/{binding}/reconnect"
            ),
            "operator",
            br#"{"idempotency_key":"reconnect-1","expected_control_generation":1,"expected_session_epoch":1}"#,
        )
        .expect("reconnect");
    let operation_id = reconnect["value"]["operation_id"]
        .as_str()
        .expect("operation id");
    let operation = route
        .handle(
            "GET",
            &format!("/v1/runs/run-fixture/provider-session-operations/{operation_id}"),
            "operator",
            &[],
        )
        .expect("operation");
    assert_eq!(operation["value"]["kind"], "reconnect");
    assert_eq!(operation["value"]["generation_class"], false);
    assert_eq!(operation["game_effects"], 0);
    let plan = route
        .handle(
            "POST",
            &format!("/v1/runs/run-fixture/provider-sessions/{binding}/fork-plans"),
            "operator",
            br#"{"idempotency_key":"fork-plan-1","expected_control_generation":1,"expected_session_epoch":1,"expected_history_epoch":0,"cutoff_turn_ref":"turn-0","operation":"clean_rehydration","purpose":"evaluation"}"#,
        )
        .expect("fork plan");
    let plan_id = plan["value"]["plan_id"].as_str().expect("plan id");
    let planned_operation = route
        .handle(
            "GET",
            &format!("/v1/runs/run-fixture/provider-session-operations/{plan_id}"),
            "operator",
            &[],
        )
        .expect("planned operation");
    assert_eq!(planned_operation["value"]["state"], "planned");
    let history = route
        .handle(
            "GET",
            &format!("/v1/runs/run-fixture/provider-sessions/{binding}/history"),
            "operator",
            &[],
        )
        .expect("history");
    assert_eq!(history["value"]["effective_context_coverage"], "unknown");
    assert_eq!(history["value"]["read_started_turn"], false);
    assert_eq!(
        route.handle(
            "GET",
            &format!("/v1/runs/other-run/provider-sessions/{binding}"),
            "operator",
            &[],
        ),
        Err(SessionApiError::NotFound)
    );
    let other_run = route
        .handle(
            "GET",
            "/v1/runs/other-run/provider-sessions",
            "operator",
            &[],
        )
        .expect("other-run list");
    assert!(
        other_run["value"]["bindings"]
            .as_array()
            .expect("bindings")
            .is_empty()
    );
}
