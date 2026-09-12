// SPDX-License-Identifier: MIT

//! Focused unit coverage for the demonstration routes and embedded assets.

use super::fixtures;
use super::routes::static_response;
use super::state::DemoState;
use crate::http::HttpRequest;

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
        fixtures::WEB_RENDER,
    ] {
        assert!(!asset.is_empty());
    }
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
