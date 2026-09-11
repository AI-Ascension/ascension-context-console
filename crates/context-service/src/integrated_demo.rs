// SPDX-License-Identifier: MIT

//! A provider-free producer → capture → read API → browser demonstration.
//!
//! The demo server intentionally serves only checked-in synthetic bytes. The `/demo/*` routes
//! adapt browser requests to the same authenticated `ReadApi` used by the service tests, so the
//! browser flow exercises the real store and API projection instead of reading fixture files
//! directly. No provider, game, URL fetch, or arbitrary process path is available here.

use crate::capture::{CaptureConfig, CaptureMode, CaptureSink, MemoryCapture, PreparedCapture};
use crate::control::{
    Command, ControlError, ControlPlane, DurableControlStore, DurableStoreError, Patch, Scope,
};
use crate::read_api::{ApiError, HttpRequest, HttpResponse, ReadApi, read_request_bytes};
use crate::store::{CapturePrivilege, IngestError, ReadGrant, Store};
use crate::{MemoryRoute, MemoryRouteError, MemoryScope};
use crate::{ProviderSessionRoute, SessionApiError};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const TOKEN: &[u8] = b"integrated-demo-token";
const PROJECT: &str = "agent-fixture-001";
const RUN: &str = "run-fixture-001";
const SNAPSHOT_ID: &str = "snapshot-fixture-metadata-001";
const COMPARISON_ID: &str = "snapshot-fixture-cli-001";
const EDITOR_TOKEN: &[u8] = b"fixture-editor-token";
const OBJECTIVE_TOKEN: &[u8] = b"fixture-objective-token";
const SESSION_TOKEN: &[u8] = b"fixture-session-token";
const SESSION_PRINCIPAL: &str = "fixture-session-reader";
const CSRF_TOKEN: &str = "fixture-csrf-token";
const DURABLE_STORE_KEY: [u8; 32] = [0x42; 32];

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
    control: ControlPlane,
    control_store: DurableControlStore,
    control_store_path: PathBuf,
    memory: MemoryRoute,
    provider_sessions: ProviderSessionRoute,
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
        let mut control = ControlPlane::synthetic();
        let nonce = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let control_store_path = std::env::temp_dir().join(format!(
            "ascension-context-console-control-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let mut control_store =
            DurableControlStore::create(&control_store_path, DURABLE_STORE_KEY, &control)
                .map_err(|_| IngestError::Capacity)?;
        control_store
            .copy_phase1_snapshot(SNAPSHOT_ID, producer_snapshot)
            .map_err(|_| IngestError::Capacity)?;
        control_store
            .copy_phase1_snapshot(COMPARISON_ID, producer_comparison)
            .map_err(|_| IngestError::Capacity)?;
        drop(control_store);
        let control_store = DurableControlStore::open(
            &control_store_path,
            DURABLE_STORE_KEY,
            control.scope().run_id.clone(),
        )
        .map_err(|_| IngestError::Capacity)?;
        control = control_store.load().map_err(|_| IngestError::Capacity)?;
        let control_scope = control.scope().clone();
        let mut memory = MemoryRoute::new(
            MemoryScope {
                project_id: control_scope.project_id,
                run_id: control_scope.run_id,
                episode_id: control_scope.episode_id,
                agent_id: control_scope.agent_id,
            },
            false,
        );
        memory.grant_search("fixture-editor-token");
        memory.grant_review("fixture-objective-token");
        let provider_sessions = ProviderSessionRoute::fixture(SESSION_PRINCIPAL);
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
            control,
            control_store,
            control_store_path,
            memory,
            provider_sessions,
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
        if request.method != "GET"
            && !path.starts_with("/v1/")
            && !path.starts_with("/v2/")
            && !path.starts_with("/v3/memory/")
        {
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
            _ if path.starts_with("/v2/") => self.control_request(request, path),
            _ if path.starts_with("/v3/memory/") => self.memory_request(request, path),
            _ if path.starts_with("/v1/runs/fixture-run-001/provider-sessions")
                || path.starts_with("/v1/runs/fixture-run-001/provider-session-") =>
            {
                self.provider_session_request(request, path)
            }
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
                ,"management_enabled":self.control.enabled()
                ,"control_status":self.control.state().status
                ,"control_events":self.control.events().len()
                ,"durable_control_store":"supported"
            }))
            .unwrap_or_else(|_| b"{}".to_vec()),
        )
    }

    fn control_request(&mut self, request: &HttpRequest, path: &str) -> HttpResponse {
        self.api_requests = self.api_requests.saturating_add(1);
        let Some(token) = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return control_error_response(ControlError::forbidden(
                "authentication_required",
                "management capability is required",
            ));
        };
        let objective_authorized = token.as_bytes() == OBJECTIVE_TOKEN;
        if token.as_bytes() != EDITOR_TOKEN && !objective_authorized {
            return control_error_response(ControlError::forbidden(
                "authentication_failed",
                "management capability is invalid",
            ));
        }
        if request.method != "GET"
            && (request.header("origin") != Some(self.expected_origin.as_str())
                || request.header("x-csrf-token") != Some(CSRF_TOKEN))
        {
            return control_error_response(ControlError::forbidden(
                "csrf_rejected",
                "write origin proof is required",
            ));
        }
        let segments = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>();
        if segments.len() < 5
            || segments[0] != "v2"
            || segments[1] != "runs"
            || segments[2] != "fixture-run"
            || segments[3] != "context-control"
        {
            return control_error_response(ControlError::invalid(
                "route_not_found",
                "management route is unavailable",
            ));
        }
        let tail = &segments[4..];
        let result = match (request.method.as_str(), tail) {
            ("GET", ["capabilities"]) => serde_json::to_value(self.capabilities())
                .map_err(|_| ControlError::invalid("encoding", "capabilities encoding failed")),
            ("GET", ["state"]) => serde_json::to_value(self.control.state())
                .map_err(|_| ControlError::invalid("encoding", "state encoding failed")),
            ("GET", ["eligible-items"]) => {
                serde_json::to_value(serde_json::json!({"items":self.control.eligible_items()}))
                    .map_err(|_| {
                        ControlError::invalid("encoding", "eligible items encoding failed")
                    })
            }
            ("GET", ["revisions"]) => {
                serde_json::to_value(serde_json::json!({"revisions":self.control.revisions()}))
                    .map_err(|_| ControlError::invalid("encoding", "revisions encoding failed"))
            }
            ("GET", ["events"]) => {
                serde_json::to_value(serde_json::json!({"events":self.control.events()}))
                    .map_err(|_| ControlError::invalid("encoding", "events encoding failed"))
            }
            ("GET", ["drafts", draft_id]) => self.control.get_draft(draft_id).and_then(|draft| {
                serde_json::to_value(draft)
                    .map_err(|_| ControlError::invalid("encoding", "draft encoding failed"))
            }),
            ("GET", ["previews", preview_id]) => {
                self.control.get_preview(preview_id).and_then(|preview| {
                    serde_json::to_value(preview)
                        .map_err(|_| ControlError::invalid("encoding", "preview encoding failed"))
                })
            }
            ("GET", ["commands", command_id]) => {
                self.control.command(command_id).and_then(|receipt| {
                    serde_json::to_value(receipt)
                        .map_err(|_| ControlError::invalid("encoding", "receipt encoding failed"))
                })
            }
            ("POST", ["drafts"]) => self.create_control_draft(&request.body),
            ("POST", ["drafts", draft_id, "operations"]) => {
                self.edit_control_draft(draft_id, &request.body, objective_authorized)
            }
            ("POST", ["previews"]) => self.create_control_preview(&request.body),
            ("POST", ["commits"]) => parse_json::<Command>(&request.body)
                .and_then(|command| self.mutate_control(|control| control.commit(command)))
                .and_then(|receipt| {
                    serde_json::to_value(receipt)
                        .map_err(|_| ControlError::invalid("encoding", "receipt encoding failed"))
                }),
            ("POST", ["pause"]) => parse_json::<Command>(&request.body)
                .and_then(|command| self.mutate_control(|control| control.pause(command)))
                .and_then(|receipt| {
                    serde_json::to_value(receipt)
                        .map_err(|_| ControlError::invalid("encoding", "receipt encoding failed"))
                }),
            ("POST", ["resume"]) => parse_json::<Command>(&request.body)
                .and_then(|command| self.mutate_control(|control| control.resume(command)))
                .and_then(|receipt| {
                    serde_json::to_value(receipt)
                        .map_err(|_| ControlError::invalid("encoding", "receipt encoding failed"))
                }),
            _ => Err(ControlError::invalid(
                "route_not_found",
                "management route is unavailable",
            )),
        };
        match result {
            Ok(value) => control_value_response(200, value),
            Err(error) => control_error_response(error),
        }
    }

    fn memory_request(&mut self, request: &HttpRequest, path: &str) -> HttpResponse {
        let Some(token) = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return memory_error_response(MemoryRouteError::PermissionDenied);
        };
        if request.method != "GET"
            && (request.header("origin") != Some(self.expected_origin.as_str())
                || request.header("x-csrf-token") != Some(CSRF_TOKEN))
        {
            return memory_error_response(MemoryRouteError::PermissionDenied);
        }
        match self
            .memory
            .handle(&request.method, path, token, &request.body)
        {
            Ok(value) => control_value_response(200, value),
            Err(error) => memory_error_response(error),
        }
    }

    fn provider_session_request(&mut self, request: &HttpRequest, path: &str) -> HttpResponse {
        self.api_requests = self.api_requests.saturating_add(1);
        let Some(token) = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
        else {
            return provider_session_error_response(SessionApiError::Unauthorized);
        };
        if token.as_bytes() != SESSION_TOKEN {
            return provider_session_error_response(SessionApiError::Unauthorized);
        }
        if request.method != "GET"
            && (request.header("origin") != Some(self.expected_origin.as_str())
                || request.header("x-csrf-token") != Some(CSRF_TOKEN))
        {
            return provider_session_error_response(SessionApiError::Forbidden);
        }
        match self
            .provider_sessions
            .handle(&request.method, path, SESSION_PRINCIPAL, &request.body)
        {
            Ok(value) => control_value_response(200, value),
            Err(error) => provider_session_error_response(error),
        }
    }

    fn create_control_draft(&mut self, body: &[u8]) -> Result<serde_json::Value, ControlError> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            scope: Scope,
            expected_active_revision_id: String,
        }
        let request: Request = parse_json(body)?;
        self.mutate_control(|control| {
            control.create_draft(
                request.scope,
                &request.expected_active_revision_id,
                "operator-fixture",
            )
        })
        .and_then(|draft| {
            serde_json::to_value(draft)
                .map_err(|_| ControlError::invalid("encoding", "draft encoding failed"))
        })
    }

    fn edit_control_draft(
        &mut self,
        draft_id: &str,
        body: &[u8],
        objective_authorized: bool,
    ) -> Result<serde_json::Value, ControlError> {
        let patch: Patch = parse_json(body)?;
        if patch.draft_id != draft_id {
            return Err(ControlError::invalid(
                "draft_mismatch",
                "draft path and body differ",
            ));
        }
        self.mutate_control(|control| {
            control.apply_patch(
                patch,
                if objective_authorized {
                    "objective-fixture"
                } else {
                    "operator-fixture"
                },
                objective_authorized,
            )
        })
        .and_then(|draft| {
            serde_json::to_value(draft)
                .map_err(|_| ControlError::invalid("encoding", "draft encoding failed"))
        })
    }

    fn create_control_preview(&mut self, body: &[u8]) -> Result<serde_json::Value, ControlError> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            scope: Scope,
            draft_id: String,
            expected_draft_version: u64,
            applicable_requested: bool,
            expected_control_version: u64,
            unknown_total_risk_acknowledged: bool,
        }
        let request: Request = parse_json(body)?;
        self.mutate_control(|control| {
            control.create_preview(
                request.scope,
                &request.draft_id,
                request.expected_draft_version,
                request.applicable_requested,
                request.expected_control_version,
                request.unknown_total_risk_acknowledged,
            )
        })
        .and_then(|preview| {
            serde_json::to_value(preview)
                .map_err(|_| ControlError::invalid("encoding", "preview encoding failed"))
        })
    }

    fn capabilities(&self) -> crate::control::Capabilities {
        let mut capabilities = self.control.capabilities();
        capabilities.durable_control_store = "supported".to_owned();
        capabilities
    }

    fn mutate_control<T>(
        &mut self,
        mutation: impl FnOnce(&mut ControlPlane) -> Result<T, ControlError>,
    ) -> Result<T, ControlError> {
        let mut candidate = self.control.clone();
        let result = mutation(&mut candidate)?;
        self.control_store
            .persist(&candidate)
            .map_err(durable_error)?;
        self.control = candidate;
        Ok(result)
    }
}

impl Drop for DemoState {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.control_store_path);
        let _ = fs::remove_file(self.control_store_path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(self.control_store_path.with_extension("sqlite-shm"));
    }
}

fn durable_error(_: DurableStoreError) -> ControlError {
    ControlError::invalid(
        "durable_store_unavailable",
        "control state could not be durably persisted",
    )
}

pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ControlError> {
    if body.is_empty() || body.len() > 16 * 1024 {
        return Err(ControlError::invalid(
            "body_too_large",
            "management body exceeds its bound",
        ));
    }
    validate_json_shape(body)?;
    serde_json::from_slice(body)
        .map_err(|_| ControlError::invalid("invalid_json", "management JSON is invalid"))
}

fn validate_json_shape(body: &[u8]) -> Result<(), ControlError> {
    let mut parser = JsonGuard {
        bytes: body,
        offset: 0,
    };
    parser.value(0)?;
    parser.space();
    if parser.offset != body.len() {
        return Err(ControlError::invalid(
            "invalid_json",
            "management JSON has trailing data",
        ));
    }
    Ok(())
}

struct JsonGuard<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl JsonGuard<'_> {
    fn value(&mut self, depth: usize) -> Result<(), ControlError> {
        if depth > 32 {
            return Err(ControlError::invalid(
                "json_depth",
                "management JSON is too deeply nested",
            ));
        }
        self.space();
        match self.bytes.get(self.offset).copied() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(_) => self.primitive(),
            None => Err(ControlError::invalid(
                "invalid_json",
                "management JSON is incomplete",
            )),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), ControlError> {
        self.offset += 1;
        self.space();
        let mut keys = BTreeSet::new();
        if self.take(b'}') {
            return Ok(());
        }
        loop {
            self.space();
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(ControlError::invalid(
                    "duplicate_json_key",
                    "duplicate JSON keys are rejected",
                ));
            }
            self.space();
            if !self.take(b':') {
                return Err(ControlError::invalid(
                    "invalid_json",
                    "object member separator is missing",
                ));
            }
            self.value(depth)?;
            self.space();
            if self.take(b'}') {
                return Ok(());
            }
            if !self.take(b',') {
                return Err(ControlError::invalid(
                    "invalid_json",
                    "object delimiter is missing",
                ));
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), ControlError> {
        self.offset += 1;
        self.space();
        if self.take(b']') {
            return Ok(());
        }
        let mut count = 0_usize;
        loop {
            count = count.saturating_add(1);
            if count > 256 {
                return Err(ControlError::invalid(
                    "json_items",
                    "management JSON array is too large",
                ));
            }
            self.value(depth)?;
            self.space();
            if self.take(b']') {
                return Ok(());
            }
            if !self.take(b',') {
                return Err(ControlError::invalid(
                    "invalid_json",
                    "array delimiter is missing",
                ));
            }
        }
    }

    fn primitive(&mut self) -> Result<(), ControlError> {
        let start = self.offset;
        while let Some(byte) = self.bytes.get(self.offset).copied() {
            if byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}') {
                break;
            }
            self.offset += 1;
        }
        (self.offset > start)
            .then_some(())
            .ok_or_else(|| ControlError::invalid("invalid_json", "JSON value is incomplete"))
    }

    fn string(&mut self) -> Result<String, ControlError> {
        let start = self.offset;
        if !self.take(b'"') {
            return Err(ControlError::invalid(
                "invalid_json",
                "JSON string is missing",
            ));
        }
        let mut escaped = false;
        while let Some(byte) = self.bytes.get(self.offset).copied() {
            self.offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return serde_json::from_slice(&self.bytes[start..self.offset])
                    .map_err(|_| ControlError::invalid("invalid_json", "JSON string is invalid"));
            }
        }
        Err(ControlError::invalid(
            "invalid_json",
            "JSON string is unterminated",
        ))
    }

    fn space(&mut self) {
        while self
            .bytes
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
        }
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.bytes.get(self.offset).copied() == Some(expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
}

fn control_value_response(status: u16, value: serde_json::Value) -> HttpResponse {
    let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"error\":\"encoding\"}".to_vec());
    static_response(status, "application/json", body)
}

fn control_error_response(error: ControlError) -> HttpResponse {
    let status = if error.code.contains("authentication") || error.code == "csrf_rejected" {
        403
    } else if error.code.contains("stale")
        || error.code.contains("conflict")
        || error.code.contains("already")
        || error.code.contains("unresolved")
        || error.code.contains("preview")
    {
        409
    } else if error.code.contains("not_found") || error.code == "route_not_found" {
        404
    } else {
        422
    };
    control_value_response(
        status,
        serde_json::json!({"error":{"code":error.code,"message":error.message}}),
    )
}

fn memory_error_response(error: MemoryRouteError) -> HttpResponse {
    let status = match error {
        MemoryRouteError::PermissionDenied => 403,
        MemoryRouteError::BodyTooLarge | MemoryRouteError::InvalidRequest => 400,
        MemoryRouteError::MethodNotAllowed => 405,
        MemoryRouteError::Unsupported => 404,
    };
    control_value_response(
        status,
        json!({
            "schema": "ascension.context-memory.error.v1",
            "error": error.to_string(),
            "effect_class": "local_read_no_inference"
        }),
    )
}

fn provider_session_error_response(error: SessionApiError) -> HttpResponse {
    let status = match error {
        SessionApiError::Unauthorized | SessionApiError::AuthNeeded => 401,
        SessionApiError::Forbidden => 403,
        SessionApiError::NotFound => 404,
        SessionApiError::MethodNotAllowed => 405,
        SessionApiError::Capacity => 429,
        SessionApiError::Conflict
        | SessionApiError::Stale
        | SessionApiError::Expired
        | SessionApiError::Ambiguous => 409,
        SessionApiError::Unavailable | SessionApiError::MalformedPeer => 503,
        SessionApiError::BadRequest | SessionApiError::Unsupported => 400,
    };
    control_value_response(
        status,
        json!({
            "schema": "ascension.provider-session.error.v1",
            "error": error.to_string(),
            "effect_class": "local_metadata_only",
            "native_calls": 0,
            "game_effects": 0
        }),
    )
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
