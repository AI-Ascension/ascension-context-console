// SPDX-License-Identifier: MIT

//! Focused unit coverage for the demonstration routes and embedded assets.

use super::fixtures;
use super::routes::static_response;
use super::state::DemoState;
use crate::http::HttpRequest;
use crate::memory::{MemoryRoute, MemoryScope};
use crate::owner::{
    HarnessOwner, HarnessOwnerComposition, OwnerCall, OwnerError, OwnerReceiptLookup, OwnerReply,
};
use crate::provider_session::ProviderSessionRoute;
use std::sync::Arc;

struct NoopOwner;

impl HarnessOwner for NoopOwner {
    fn call(&self, _request: OwnerCall) -> Result<OwnerReply, OwnerError> {
        Err(OwnerError::Unavailable)
    }

    fn lookup_receipt(&self, _request: OwnerReceiptLookup) -> Result<OwnerReply, OwnerError> {
        Err(OwnerError::Unavailable)
    }
}

fn get(target: &str) -> HttpRequest {
    HttpRequest {
        method: "GET".to_owned(),
        target: target.to_owned(),
        headers: Vec::new(),
        body: Vec::new(),
    }
}

#[test]
fn checked_in_review_assets_are_non_empty() {
    for asset in [
        fixtures::CLI_SNAPSHOT,
        fixtures::METADATA_SNAPSHOT,
        fixtures::EVENTS,
        fixtures::WEB_INDEX,
        fixtures::WEB_STYLES,
        fixtures::WEB_APP,
        fixtures::WEB_API,
        fixtures::WEB_BUNDLE,
        fixtures::WEB_POLICY_OWNER,
        fixtures::WEB_RENDER,
    ] {
        assert!(!asset.is_empty());
    }
}

#[test]
fn policy_owner_module_is_served_from_the_review_surface() {
    let mut state = DemoState::build(0).expect("demo state");
    let response = state.dispatch(&get("/web/js/policy-owner.js"));
    assert_eq!(response.status, 200);
    assert_eq!(response.body, fixtures::WEB_POLICY_OWNER);
    assert!(
        response.headers.iter().any(
            |(name, value)| name == "Content-Type" && value == "text/javascript; charset=utf-8"
        )
    );
}

#[test]
fn static_response_is_local_and_unbuffered() {
    let response = static_response(200, "text/plain", b"ok".to_vec());
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"ok");
    assert!(
        response
            .headers
            .iter()
            .any(|(name, value)| name == "Cache-Control" && value == "no-store")
    );
    assert!(
        response
            .headers
            .iter()
            .any(|(name, value)| name == "Content-Length" && value == "2")
    );
}

#[test]
fn unknown_route_is_not_found() {
    let mut state = DemoState::build(0).expect("demo state");
    let response = state.dispatch(&get("/missing"));
    assert_eq!(response.status, 404);
}

#[test]
fn non_get_route_is_method_not_allowed() {
    let mut state = DemoState::build(0).expect("demo state");
    let mut request = get("/demo/metrics");
    request.method = "POST".to_owned();
    let response = state.dispatch(&request);
    assert_eq!(response.status, 405);
}

#[test]
fn metrics_report_a_provider_free_read_only_transport() {
    let mut state = DemoState::build(0).expect("demo state");
    let response = state.dispatch(&get("/demo/metrics"));
    assert_eq!(response.status, 200);
    let value: serde_json::Value = serde_json::from_slice(&response.body).expect("metrics json");
    assert_eq!(value["provider_calls"], 0);
    assert_eq!(value["game_launches"], 0);
    assert_eq!(value["external_requests"], 0);
    assert_eq!(value["read_only"].as_bool(), Some(true));
}

#[test]
fn attached_memory_does_not_disable_control_csrf() {
    let mut state = DemoState::build(0).expect("demo state");
    state.memory = MemoryRoute::attached(
        MemoryScope {
            project_id: "fixture-project".to_owned(),
            run_id: "fixture-run".to_owned(),
            episode_id: "fixture-episode".to_owned(),
            agent_id: "fixture-agent".to_owned(),
        },
        HarnessOwnerComposition::new(Arc::new(NoopOwner), Default::default()),
    );
    let response = state.dispatch(&HttpRequest {
        method: "POST".to_owned(),
        target: "/v2/runs/fixture-run/context-control/pause".to_owned(),
        headers: vec![(
            "authorization".to_owned(),
            "Bearer fixture-editor-token".to_owned(),
        )],
        body: Vec::new(),
    });
    assert_eq!(response.status, 403);
}

#[test]
fn attached_provider_session_does_not_disable_memory_csrf() {
    let mut state = DemoState::build(0).expect("demo state");
    state.provider_sessions = ProviderSessionRoute::attached(
        super::SESSION_PRINCIPAL,
        HarnessOwnerComposition::new(Arc::new(NoopOwner), Default::default()),
    );
    let response = state.dispatch(&HttpRequest {
        method: "POST".to_owned(),
        target: "/v3/memory/search".to_owned(),
        headers: vec![(
            "authorization".to_owned(),
            "Bearer fixture-editor-token".to_owned(),
        )],
        body: Vec::new(),
    });
    assert_eq!(response.status, 403);
}
