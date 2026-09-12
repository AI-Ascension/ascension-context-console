// SPDX-License-Identifier: MIT

mod support;

use context_service::{
    ApiError, CapturePrivilege, HttpRequest, HttpResponse, ReadApi, ReadGrant, Store,
};
use serde_json::Value;
use std::time::{Duration, UNIX_EPOCH};
use support::fixtures::read_fixture;

const HOST: &str = "127.0.0.1:7878";
const ORIGIN: &str = "http://127.0.0.1:7878";
const TOKEN: &[u8] = b"read-api-token";

fn store_and_grant() -> (Store, ReadGrant) {
    let mut store = Store::default();
    store
        .ingest(&read_fixture("fixtures/synthetic/snapshot.json"))
        .expect("snapshot");
    let grant = ReadGrant::issue(
        TOKEN,
        "agent-t02-reader",
        Some("run-t02-001".to_owned()),
        CapturePrivilege::Metadata,
        Duration::from_secs(60),
        UNIX_EPOCH,
    )
    .expect("grant");
    (store, grant)
}

fn request(method: &str, target: &str, authenticated: bool) -> HttpRequest {
    let mut headers = vec![
        ("host".to_owned(), HOST.to_owned()),
        ("origin".to_owned(), ORIGIN.to_owned()),
    ];
    if authenticated {
        headers.push((
            "authorization".to_owned(),
            format!("Bearer {}", std::str::from_utf8(TOKEN).expect("token")),
        ));
    }
    HttpRequest {
        method: method.to_owned(),
        target: target.to_owned(),
        headers,
        body: Vec::new(),
    }
}

fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
}

fn json(response: &HttpResponse) -> Value {
    serde_json::from_slice(&response.body).expect("json body")
}

#[test]
fn health_and_capabilities_are_read_only_and_no_store() {
    let (store, grant) = store_and_grant();
    let api = ReadApi::new(
        &store,
        &grant,
        TOKEN,
        HOST,
        Some(ORIGIN.to_owned()),
        UNIX_EPOCH,
    );

    let health = api.handle_at(&request("GET", "/health", false), UNIX_EPOCH);
    assert_eq!(health.status, 200);
    assert_eq!(header(&health, "cache-control"), Some("no-store"));
    assert_eq!(header(&health, "content-type"), Some("application/json"));
    let body = json(&health);
    assert_eq!(body["status"], "ok");
    assert!(body["read_only"].as_bool().unwrap_or(false));
    assert_eq!(body["provider_calls"], 0);
    assert_eq!(body["game_launches"], 0);

    let capabilities = api.handle_at(&request("GET", "/v1/capabilities", true), UNIX_EPOCH);
    assert_eq!(capabilities.status, 200);
    assert_eq!(header(&capabilities, "cache-control"), Some("no-store"));
    assert_eq!(
        header(&capabilities, "x-content-type-options"),
        Some("nosniff")
    );
    assert!(json(&capabilities).is_object());
}

#[test]
fn unauthenticated_and_mutating_requests_are_rejected() {
    let (store, grant) = store_and_grant();
    let api = ReadApi::new(
        &store,
        &grant,
        TOKEN,
        HOST,
        Some(ORIGIN.to_owned()),
        UNIX_EPOCH,
    );

    let unauthorized = api.handle_at(&request("GET", "/v1/capabilities", false), UNIX_EPOCH);
    assert_eq!(unauthorized.status, 401);
    assert_eq!(header(&unauthorized, "cache-control"), Some("no-store"));

    let post = api.handle_at(&request("POST", "/v1/capabilities", true), UNIX_EPOCH);
    assert_eq!(post.status, 405);
    assert!(json(&post)["read_only"].as_bool().unwrap_or(false));

    let mut with_body = request("GET", "/health", true);
    with_body.body = b"not allowed".to_vec();
    let body = api.handle_at(&with_body, UNIX_EPOCH);
    assert_eq!(body.status, 400);
    assert_eq!(json(&body)["error"], "invalid_request_framing");
}

#[test]
fn security_boundaries_reject_bad_host_origin_and_url_tokens() {
    let (store, grant) = store_and_grant();
    let api = ReadApi::new(
        &store,
        &grant,
        TOKEN,
        HOST,
        Some(ORIGIN.to_owned()),
        UNIX_EPOCH,
    );

    let mut bad_host = request("GET", "/v1/capabilities", true);
    bad_host.headers[0].1 = "evil.invalid".to_owned();
    assert_eq!(api.handle_at(&bad_host, UNIX_EPOCH).status, 400);

    let mut bad_origin = request("GET", "/v1/capabilities", true);
    bad_origin.headers[1].1 = "https://evil.invalid".to_owned();
    assert_eq!(api.handle_at(&bad_origin, UNIX_EPOCH).status, 403);

    let token_in_url = api.handle_at(
        &request("GET", "/v1/capabilities?token=read-api-token", true),
        UNIX_EPOCH,
    );
    assert_eq!(token_in_url.status, 400);

    let escaped_token = api.handle_at(
        &request("GET", "/v1/capabilities?%74oken=read-api-token", true),
        UNIX_EPOCH,
    );
    assert_eq!(escaped_token.status, 400);
}

#[test]
fn runs_snapshot_and_component_projections_round_trip() {
    let (store, grant) = store_and_grant();
    let api = ReadApi::new(
        &store,
        &grant,
        TOKEN,
        HOST,
        Some(ORIGIN.to_owned()),
        UNIX_EPOCH,
    );
    let synthetic = read_fixture("fixtures/synthetic/snapshot.json");

    let runs = api.handle_at(&request("GET", "/v1/runs", true), UNIX_EPOCH);
    assert_eq!(runs.status, 200);
    assert_eq!(json(&runs)["runs"], serde_json::json!(["run-t02-001"]));

    let snapshots = api.handle_at(
        &request("GET", "/v1/runs/run-t02-001/snapshots", true),
        UNIX_EPOCH,
    );
    assert_eq!(snapshots.status, 200);
    assert_eq!(
        json(&snapshots)["snapshots"][0]["snapshot_id"],
        "snapshot-t02-synthetic-001"
    );

    let snapshot = api.handle_at(
        &request(
            "GET",
            "/v1/runs/run-t02-001/snapshots/snapshot-t02-synthetic-001",
            true,
        ),
        UNIX_EPOCH,
    );
    assert_eq!(snapshot.status, 200);
    assert_eq!(header(&snapshot, "content-type"), Some("application/json"));
    assert_eq!(header(&snapshot, "cache-control"), Some("no-store"));
    assert_eq!(snapshot.body, synthetic);

    let component = api.handle_at(
        &request(
            "GET",
            "/v1/runs/run-t02-001/snapshots/snapshot-t02-synthetic-001/components/component-t02-body",
            true,
        ),
        UNIX_EPOCH,
    );
    assert_eq!(component.status, 200);
    let body = json(&component);
    assert_eq!(body["component"]["component_id"], "component-t02-body");
    assert_eq!(body["content"], "content_not_authorized");
}

#[test]
fn unknown_resources_and_expired_grants_fail_closed() {
    let (store, grant) = store_and_grant();
    let api = ReadApi::new(
        &store,
        &grant,
        TOKEN,
        HOST,
        Some(ORIGIN.to_owned()),
        UNIX_EPOCH,
    );

    assert_eq!(
        api.handle_at(&request("GET", "/v1/unknown", true), UNIX_EPOCH)
            .status,
        404
    );
    assert_eq!(
        api.handle_at(&request("GET", "/v1/../secret", true), UNIX_EPOCH)
            .status,
        400
    );
    assert_eq!(
        api.handle_at(
            &request("GET", "/v1/capabilities", true),
            UNIX_EPOCH + Duration::from_secs(60),
        )
        .status,
        401
    );

    let wrong_run = api.handle_at(
        &request(
            "GET",
            "/v1/runs/run-other/snapshots/snapshot-t02-synthetic-001",
            true,
        ),
        UNIX_EPOCH,
    );
    assert_eq!(wrong_run.status, 404);
}

#[test]
fn parsed_http_requests_flow_through_the_api() {
    let (store, grant) = store_and_grant();
    let api = ReadApi::new(
        &store,
        &grant,
        TOKEN,
        HOST,
        Some(ORIGIN.to_owned()),
        UNIX_EPOCH,
    );

    let raw = format!(
        "GET /v1/capabilities HTTP/1.1\r\nHost: {HOST}\r\nOrigin: {ORIGIN}\r\nAuthorization: Bearer {}\r\n\r\n",
        std::str::from_utf8(TOKEN).expect("token")
    );
    let parsed = HttpRequest::parse(raw.as_bytes()).expect("parse");
    assert_eq!(parsed.method, "GET");
    assert_eq!(parsed.target, "/v1/capabilities");
    assert_eq!(api.handle_at(&parsed, UNIX_EPOCH).status, 200);

    assert_eq!(
        HttpRequest::parse(b"GET /v1/capabilities HTTP/1.1\r\nHost: 127.0.0.1:7878"),
        Err(ApiError::BadRequest)
    );
}
