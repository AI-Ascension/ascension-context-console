// SPDX-License-Identifier: MIT

//! A provider-free producer → capture → read API → browser demonstration.
//!
//! The demo server intentionally serves only checked-in synthetic bytes. The `/demo/*` routes
//! adapt browser requests to the same authenticated `ReadApi` used by the service tests, so the
//! browser flow exercises the real store and API projection instead of reading fixture files
//! directly. No provider, game, URL fetch, or arbitrary process path is available here.

use crate::capture::{CaptureConfig, CaptureMode, CaptureSink, MemoryCapture, PreparedCapture};
use crate::read_api::{ApiError, HttpRequest, HttpResponse, ReadApi, read_request_bytes};
use crate::store::{CapturePrivilege, IngestError, ReadGrant, Store};
use serde_json::{Value, json};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, SystemTime};

const TOKEN: &[u8] = b"integrated-demo-token";
const PROJECT: &str = "agent-fixture-001";
const RUN: &str = "run-fixture-001";
const SNAPSHOT_ID: &str = "snapshot-fixture-metadata-001";
const COMPARISON_ID: &str = "snapshot-fixture-cli-001";

/// Run the bounded integrated demonstration until the operator terminates the process.
pub fn run(port: u16) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let mut state = DemoState::build(address.port()).map_err(|error| error.to_string())?;
    println!("integrated_demo_ready=http://{address}/web/");
    println!("integrated_demo_provider_calls=0");
    println!("integrated_demo_game_launches=0");
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;

    for incoming in listener.incoming() {
        let mut stream = incoming.map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let response = match read_request(&mut stream) {
            Ok(request) => state.dispatch(&request),
            Err(error) => error_response(error),
        };
        response
            .write_to(&mut stream)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct DemoState {
    store: Store,
    grant: ReadGrant,
    token: Vec<u8>,
    expected_host: String,
    expected_origin: String,
    producer_snapshots: usize,
    capture_records: usize,
    producer_events: usize,
    browser_requests: usize,
    api_requests: usize,
}

impl DemoState {
    fn build(port: u16) -> Result<Self, IngestError> {
        let producer_snapshot = include_bytes!("../../../fixtures/valid/snapshot-metadata.json");
        let producer_comparison = include_bytes!("../../../fixtures/valid/snapshot-cli.json");
        let producer_events = include_bytes!("../../../fixtures/valid/events.jsonl");

        // Producer stage: the bytes are the checked-in synthetic projection. Capture stage:
        // MemoryCapture receives those exact bytes before the store validates and retains them.
        let mut capture = MemoryCapture::new(CaptureConfig {
            mode: CaptureMode::Memory,
            max_queue_entries: 16,
            max_record_bytes: 1_048_576,
        })
        .map_err(|_| IngestError::Capacity)?;
        capture
            .prepared(PreparedCapture {
                snapshot_id: SNAPSHOT_ID,
                attempt_id: "attempt-fixture-001",
                boundary: "adapter.cli_input",
                bytes: producer_snapshot,
            })
            .map_err(|_| IngestError::Capacity)?;
        capture
            .write_completed(SNAPSHOT_ID)
            .map_err(|_| IngestError::Capacity)?;

        let mut store = Store::default();
        store.ingest(producer_snapshot)?;
        store.ingest(producer_comparison)?;
        let mut producer_event_count = 0_usize;
        for line in producer_events
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            store.append_event(line)?;
            producer_event_count += 1;
        }
        let now = SystemTime::now();
        let grant = ReadGrant::issue(
            TOKEN,
            PROJECT,
            Some(RUN.to_owned()),
            CapturePrivilege::Content,
            Duration::from_secs(3600),
            now,
        )
        .map_err(|_| IngestError::Capacity)?;
        Ok(Self {
            store,
            grant,
            token: TOKEN.to_vec(),
            expected_host: format!("127.0.0.1:{port}"),
            expected_origin: format!("http://127.0.0.1:{port}"),
            producer_snapshots: 2,
            capture_records: capture.records().count(),
            producer_events: producer_event_count,
            browser_requests: 0,
            api_requests: 0,
        })
    }

    fn dispatch(&mut self, request: &HttpRequest) -> HttpResponse {
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
                include_bytes!("../../../web/index.html").to_vec(),
            ),
            "/web/app.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                include_bytes!("../../../web/app.js").to_vec(),
            ),
            "/web/styles.css" => static_response(
                200,
                "text/css; charset=utf-8",
                include_bytes!("../../../web/styles.css").to_vec(),
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

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, ApiError> {
    let bytes = read_request_bytes(stream)?;
    HttpRequest::parse(&bytes)
}

fn static_response(status: u16, media_type: &str, body: Vec<u8>) -> HttpResponse {
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

fn error_response(error: ApiError) -> HttpResponse {
    let (status, code) = match error {
        ApiError::BadRequest => (400, "bad_request"),
        ApiError::Unauthorized => (401, "unauthorized"),
        ApiError::Forbidden => (403, "forbidden"),
        ApiError::NotFound => (404, "not_found"),
        ApiError::MethodNotAllowed => (405, "method_not_allowed"),
        ApiError::TooLarge | ApiError::TooManyRequests => (429, "bounded_limit"),
        ApiError::Io => (500, "local_io"),
    };
    static_response(
        status,
        "application/json",
        serde_json::to_vec(&json!({"error":code,"read_only":true}))
            .unwrap_or_else(|_| br#"{"error":"local_io"}"#.to_vec()),
    )
}

fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}
