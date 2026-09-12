// SPDX-License-Identifier: MIT

//! Same-origin routing for the loopback demonstration server.
//!
//! `/demo/*` adapts browser requests to the authenticated `ReadApi`; `/web/*` and
//! `/offline-bundle.json` serve the checked-in static review surface. Every response is
//! read-only and served from local bytes.

use super::fixtures;
use super::state::DemoState;
use super::{COMPARISON_ID, RUN, SNAPSHOT_ID};
use crate::http::{HttpRequest, HttpResponse, split_target};
use crate::read_api::ReadApi;
use serde_json::{Value, json};
use std::time::SystemTime;

impl DemoState {
    pub(super) fn dispatch(&mut self, request: &HttpRequest) -> HttpResponse {
        self.browser_requests = self.browser_requests.saturating_add(1);
        let (path, _) = split_target(&request.target);
        if request.method == "GET" && !request.body.is_empty() {
            return static_response(
                400,
                "application/json",
                br#"{"error":"get_body_not_allowed","read_only":true}"#.to_vec(),
            );
        }
        if request.method != "GET" && !path.starts_with("/v1/") {
            return static_response(
                405,
                "application/json",
                br#"{"error":"method_not_allowed","read_only":true}"#.to_vec(),
            );
        }
        match path {
            "/web/" | "/web/index.html" => static_response(
                200,
                "text/html; charset=utf-8",
                fixtures::WEB_INDEX.to_vec(),
            ),
            "/web/css/styles.css" => static_response(
                200,
                "text/css; charset=utf-8",
                fixtures::WEB_STYLES.to_vec(),
            ),
            "/web/js/app.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                fixtures::WEB_APP.to_vec(),
            ),
            "/web/js/api.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                fixtures::WEB_API.to_vec(),
            ),
            "/web/js/bundle.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                fixtures::WEB_BUNDLE.to_vec(),
            ),
            "/web/js/render.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                fixtures::WEB_RENDER.to_vec(),
            ),
            "/offline-bundle.json" => self.manifest_response(),
            "/demo/snapshot" => self.api_snapshot(SNAPSHOT_ID),
            "/demo/comparison" => self.api_snapshot(COMPARISON_ID),
            "/demo/events" => self.api_events(),
            "/demo/metrics" => self.metrics_response(),
            _ if path.starts_with("/v1/") => self.api_request(request),
            _ => static_response(
                404,
                "application/json",
                br#"{"error":"not_found"}"#.to_vec(),
            ),
        }
    }

    fn manifest_response(&self) -> HttpResponse {
        let body = serde_json::to_vec(&json!({
            "schema":"ascension.offline-bundle.v1",
            "evidence":"synthetic",
            "snapshot":"demo/snapshot",
            "comparison":"demo/comparison",
            "events":"demo/events"
        }))
        .unwrap_or_else(|_| b"{}".to_vec());
        static_response(200, "application/json", body)
    }

    fn api_snapshot(&mut self, snapshot_id: &str) -> HttpResponse {
        let target = format!("/v1/runs/{RUN}/snapshots/{snapshot_id}");
        self.api_projection(&target)
    }

    fn api_events(&mut self) -> HttpResponse {
        let response = self.api_projection(&format!("/v1/runs/{RUN}/events"));
        if response.status != 200 {
            return response;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&response.body) else {
            return static_response(
                502,
                "application/json",
                br#"{"error":"invalid_api_projection"}"#.to_vec(),
            );
        };
        let Some(events) = value.get("events").and_then(Value::as_array) else {
            return static_response(
                502,
                "application/json",
                br#"{"error":"invalid_api_projection"}"#.to_vec(),
            );
        };
        let mut body = Vec::new();
        for event in events {
            let mut event = event.clone();
            if let Some(object) = event.as_object_mut() {
                object.insert(
                    "schema".to_owned(),
                    Value::String("ascension.context-event.v1".to_owned()),
                );
            }
            let Ok(line) = serde_json::to_vec(&event) else {
                return static_response(
                    502,
                    "application/json",
                    br#"{"error":"invalid_api_projection"}"#.to_vec(),
                );
            };
            body.extend_from_slice(&line);
            body.push(b'\n');
        }
        static_response(200, "application/x-ndjson", body)
    }

    fn api_projection(&mut self, target: &str) -> HttpResponse {
        self.api_requests = self.api_requests.saturating_add(1);
        let request = HttpRequest {
            method: "GET".to_owned(),
            target: target.to_owned(),
            headers: vec![
                ("host".to_owned(), self.expected_host.clone()),
                ("origin".to_owned(), self.expected_origin.clone()),
                (
                    "authorization".to_owned(),
                    format!("Bearer {}", String::from_utf8_lossy(&self.token)),
                ),
            ],
            body: Vec::new(),
        };
        let api = ReadApi::new(
            &self.store,
            &self.grant,
            &self.token,
            self.expected_host.clone(),
            Some(self.expected_origin.clone()),
            SystemTime::now(),
        );
        api.handle(&request)
    }

    fn api_request(&mut self, request: &HttpRequest) -> HttpResponse {
        self.api_requests = self.api_requests.saturating_add(1);
        let api = ReadApi::new(
            &self.store,
            &self.grant,
            &self.token,
            self.expected_host.clone(),
            Some(self.expected_origin.clone()),
            SystemTime::now(),
        );
        api.handle(request)
    }

    fn metrics_response(&self) -> HttpResponse {
        static_response(
            200,
            "application/json",
            serde_json::to_vec(&json!({
                "schema":"ascension.integrated-demo-metrics.v1",
                "producer_snapshots":self.producer_snapshots,
                "capture_records":self.capture_records,
                "producer_events":self.producer_events,
                "browser_requests":self.browser_requests,
                "api_requests":self.api_requests,
                "provider_calls":0,
                "game_launches":0,
                "external_requests":0,
                "read_only":true
            }))
            .unwrap_or_else(|_| b"{}".to_vec()),
        )
    }
}

pub(super) fn static_response(status: u16, media_type: &str, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![
            ("Content-Type".to_owned(), media_type.to_owned()),
            ("Cache-Control".to_owned(), "no-store".to_owned()),
            ("Content-Length".to_owned(), body.len().to_string()),
            ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
        ],
        body,
    }
}
