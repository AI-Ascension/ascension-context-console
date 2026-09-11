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
    assert_eq!(value["value"]["hardening"]["encrypted_state"], false);
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
fn binding_routes_cannot_cross_event_or_operation_prefixes() {
    let request = br#"{"idempotency_key":"candidate-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation"}"#;
    let mut route = ProviderSessionRoute::fixture("operator");
    for path in [
        "/v1/runs/run-fixture/provider-session-events/candidates",
        "/v1/runs/run-fixture/provider-session-operations/candidates",
    ] {
        assert!(matches!(
            route.handle("POST", path, "operator", request),
            Err(SessionApiError::MethodNotAllowed | SessionApiError::NotFound)
        ));
    }
    let candidate = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            request,
        )
        .expect("candidate");
    let binding = candidate["value"]["binding_id"].as_str().expect("binding");
    for path in [
        format!("/v1/runs/run-fixture/provider-session-events/{binding}"),
        format!("/v1/runs/run-fixture/provider-session-events/{binding}/history"),
    ] {
        assert_eq!(
            route.handle("GET", &path, "operator", &[]),
            Err(SessionApiError::NotFound)
        );
    }
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

#[test]
fn authority_bearing_or_raw_rpc_input_is_rejected_without_effect() {
    let mut route = ProviderSessionRoute::fixture("operator");
    for body in [
        br#"{"idempotency_key":"authority-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","method":"thread/start"}"#.as_slice(),
        br#"{"idempotency_key":"authority-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","native_method":"turn/start"}"#.as_slice(),
        br#"{"idempotency_key":"authority-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","role":"system"}"#.as_slice(),
        br#"{"idempotency_key":"authority-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","rpc":{"jsonrpc":"2.0","method":"turn/start"}}"#.as_slice(),
    ] {
        assert_eq!(
            route.handle(
                "POST",
                "/v1/runs/run-fixture/provider-sessions/candidates",
                "operator",
                body,
            ),
            Err(SessionApiError::BadRequest)
        );
    }
    // No candidate was created by any rejected authority-bearing request.
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
        0
    );

    let candidate = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            br#"{"idempotency_key":"authority-2","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation"}"#,
        )
        .expect("candidate");
    let binding = candidate["value"]["binding_id"]
        .as_str()
        .expect("binding id");
    for suffix in ["turn/start", "rpc", "native-rpc"] {
        assert_eq!(
            route.handle(
                "POST",
                &format!("/v1/runs/run-fixture/provider-sessions/{binding}/{suffix}"),
                "operator",
                b"{}",
            ),
            Err(SessionApiError::NotFound)
        );
    }
    assert_eq!(
        route.handle(
            "POST",
            "/v1/runs/run-fixture/provider-session-operations",
            "operator",
            b"{}",
        ),
        Err(SessionApiError::MethodNotAllowed)
    );
}

#[test]
fn diagnostics_do_not_leak_paths_or_credentials_and_do_not_echo_input() {
    let errors = [
        SessionApiError::BadRequest,
        SessionApiError::Unauthorized,
        SessionApiError::AuthNeeded,
        SessionApiError::Forbidden,
        SessionApiError::NotFound,
        SessionApiError::MethodNotAllowed,
        SessionApiError::Unsupported,
        SessionApiError::Capacity,
        SessionApiError::Conflict,
        SessionApiError::Stale,
        SessionApiError::Expired,
        SessionApiError::Ambiguous,
        SessionApiError::Unavailable,
        SessionApiError::MalformedPeer,
    ];
    for error in errors {
        let text = error.to_string().to_lowercase();
        for forbidden in ["/", "\\", "secret", "bearer", "token", "openai", "http"] {
            assert!(
                !text.contains(forbidden),
                "diagnostic {error:?} leaked {forbidden}: {text}"
            );
        }
    }

    let secret = "/home/agent/.codex/secret-token";
    let body = format!(
        r#"{{"idempotency_key":"leak-1","expected_control_generation":1,"approved_policy_ref":"{secret}","profile_ref":"profile-1","purpose":"evaluation"}}"#
    );
    let mut route = ProviderSessionRoute::fixture("operator");
    let error = route
        .handle(
            "POST",
            "/v1/runs/run-fixture/provider-sessions/candidates",
            "operator",
            body.as_bytes(),
        )
        .expect_err("invalid policy ref must be rejected");
    assert_eq!(error, SessionApiError::BadRequest);
    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}

#[test]
fn phase4_cli_rejects_authority_input_without_echoing_secrets() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let secret = "/home/agent/.codex/secret-token";
    let body = format!(
        r#"{{"idempotency_key":"cli-1","expected_control_generation":1,"approved_policy_ref":"policy-1","profile_ref":"profile-1","purpose":"evaluation","native_method":"{secret}"}}"#
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_context-console"))
        .args(["phase4-cli", "candidate"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn context-console");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(body.as_bytes())
        .expect("write body");
    let output = child.wait_with_output().expect("wait");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("provider-session request is invalid"));
    assert!(!stderr.contains(secret));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(secret));
}
