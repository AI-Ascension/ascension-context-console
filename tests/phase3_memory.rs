// SPDX-License-Identifier: MIT

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use context_service::{MemoryRoute, MemoryRouteError, MemoryScope, run_phase3_cli};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

fn scope() -> MemoryScope {
    MemoryScope {
        project_id: "project-fixture".to_owned(),
        run_id: "run-fixture".to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    }
}

#[test]
fn route_enforces_separate_permissions_and_closed_query_shape() {
    let mut route = MemoryRoute::new(scope(), false);
    route.grant_search("operator");
    assert_eq!(
        route.handle("POST", "/v3/memory/generate", "operator", b"{}"),
        Err(MemoryRouteError::PermissionDenied)
    );
    let query = serde_json::json!({
        "schema": "ascension.context-memory.query.v1",
        "scope": scope(),
        "branch_id": "branch-a",
        "query": "HP settled",
        "cutoff": 10,
        "corpus_generation": 10,
        "ranker_version": "lexical-v1",
        "limit": 8,
        "max_candidates": 64,
        "effect_class": "local_read_no_inference"
    });
    let response = route
        .handle(
            "POST",
            "/v3/memory/search",
            "operator",
            &serde_json::to_vec(&query).expect("query"),
        )
        .expect("search");
    assert_eq!(response["inference_calls"], 0);
    assert_eq!(response["coverage"], "projection_unavailable");
}

#[test]
fn compiled_phase3_cli_exposes_capabilities_and_bounded_search() {
    let capabilities = Command::new(env!("CARGO_BIN_EXE_context-console"))
        .args(["phase3-cli", "capabilities"])
        .output()
        .expect("capabilities");
    assert!(capabilities.status.success());
    let value: Value = serde_json::from_slice(&capabilities.stdout).expect("capability JSON");
    assert_eq!(value["schema"], "ascension.context-memory.cli-result.v1");
    assert_eq!(value["value"]["phase2_approval_required"], true);
    let query = serde_json::json!({
        "schema": "ascension.context-memory.query.v1",
        "scope": scope(),
        "branch_id": "branch-a",
        "query": "HP settled",
        "cutoff": 10,
        "corpus_generation": 10,
        "ranker_version": "lexical-v1",
        "limit": 8,
        "max_candidates": 64,
        "effect_class": "local_read_no_inference"
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_context-console"))
        .args(["phase3-cli", "search"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("search process");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(&serde_json::to_vec(&query).expect("query bytes"))
        .expect("write query");
    let output = child.wait_with_output().expect("search output");
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).expect("search JSON");
    assert_eq!(value["value"]["inference_calls"], 0);
}

#[test]
fn phase3_cli_help_is_available_without_a_store() {
    run_phase3_cli(vec!["help".to_owned()]).expect("help");
}
