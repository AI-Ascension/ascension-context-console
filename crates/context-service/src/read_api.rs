// SPDX-License-Identifier: MIT

//! Small loopback read surface.  It parses a bounded HTTP request, authenticates a bearer
//! capability from the header, and dispatches only GET resources backed by the immutable store.
//! There is no process, provider, game, URL-fetch or mutation path in this module.

use crate::store::{CapturePrivilege, ReadError, ReadGrant, Store};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_HTTP_REQUEST_BYTES: usize = 16 * 1024;
pub const MAX_HTTP_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpRequest {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self, ApiError> {
        if bytes.is_empty() || bytes.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(ApiError::BadRequest);
        }
        let split = bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or(ApiError::BadRequest)?;
        let (head, body) = bytes.split_at(split);
        let body = &body[4..];
        if body.len() > MAX_HTTP_BODY_BYTES {
            return Err(ApiError::TooLarge);
        }
        let mut lines = head.split(|byte| *byte == b'\n');
        let request_line = lines.next().ok_or(ApiError::BadRequest)?;
        let request_line = trim_cr(request_line);
        let mut parts = request_line.split(|byte| *byte == b' ');
        let method = ascii_token(parts.next().ok_or(ApiError::BadRequest)?)?;
        let target = std::str::from_utf8(parts.next().ok_or(ApiError::BadRequest)?)
            .map_err(|_| ApiError::BadRequest)?
            .to_owned();
        let version = parts.next().ok_or(ApiError::BadRequest)?;
        if version != b"HTTP/1.1" || parts.next().is_some() {
            return Err(ApiError::BadRequest);
        }
        if target.is_empty() || target.len() > 4096 || target.contains('\0') {
            return Err(ApiError::BadRequest);
        }
        let mut headers = Vec::new();
        for line in lines {
            let line = trim_cr(line);
            if line.is_empty() {
                continue;
            }
            let colon = line
                .iter()
                .position(|byte| *byte == b':')
                .ok_or(ApiError::BadRequest)?;
            let name = std::str::from_utf8(&line[..colon]).map_err(|_| ApiError::BadRequest)?;
            let value =
                std::str::from_utf8(&line[colon + 1..]).map_err(|_| ApiError::BadRequest)?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || name.len() > 64 || value.len() > 2048 {
                return Err(ApiError::BadRequest);
            }
            if !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-".contains(&byte))
                || value.chars().any(char::is_control)
            {
                return Err(ApiError::BadRequest);
            }
            headers.push((name.to_ascii_lowercase(), value.to_owned()));
            if headers.len() > 32 {
                return Err(ApiError::TooLarge);
            }
        }
        Ok(Self {
            method,
            target,
            headers,
            body: body.to_vec(),
        })
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, value: Value) -> Self {
        let body =
            serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"error\":\"encoding\"}".to_vec());
        Self {
            status,
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Cache-Control".to_owned(), "no-store".to_owned()),
                ("Content-Length".to_owned(), body.len().to_string()),
                ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
            ],
            body,
        }
    }

    fn raw_json(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Cache-Control".to_owned(), "no-store".to_owned()),
                ("Content-Length".to_owned(), body.len().to_string()),
                ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
            ],
            body,
        }
    }

    fn content(status: u16, media_type: &str, body: Vec<u8>) -> Self {
        Self {
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

    pub fn write_to(&self, stream: &mut TcpStream) -> Result<(), ApiError> {
        let reason = match self.status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            429 => "Too Many Requests",
            _ => "Internal Server Error",
        };
        write!(stream, "HTTP/1.1 {} {}\r\n", self.status, reason).map_err(|_| ApiError::Io)?;
        for (name, value) in &self.headers {
            write!(stream, "{name}: {value}\r\n").map_err(|_| ApiError::Io)?;
        }
        stream
            .write_all(b"Connection: close\r\n\r\n")
            .map_err(|_| ApiError::Io)?;
        stream.write_all(&self.body).map_err(|_| ApiError::Io)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiError {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    TooLarge,
    TooManyRequests,
    Io,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BadRequest => "bad request",
            Self::Unauthorized => "read capability required",
            Self::Forbidden => "read capability does not permit this resource",
            Self::NotFound => "resource unavailable",
            Self::MethodNotAllowed => "only GET is supported",
            Self::TooLarge => "request exceeds its bound",
            Self::TooManyRequests => "bounded read capacity exceeded",
            Self::Io => "local transport error",
        })
    }
}

impl std::error::Error for ApiError {}

pub struct ReadApi<'a> {
    store: &'a Store,
    grant: &'a ReadGrant,
    token: &'a [u8],
    expected_host: String,
    expected_origin: Option<String>,
}

impl<'a> ReadApi<'a> {
    pub fn new(
        store: &'a Store,
        grant: &'a ReadGrant,
        token: &'a [u8],
        expected_host: impl Into<String>,
        expected_origin: Option<String>,
        _now: SystemTime,
    ) -> Self {
        Self {
            store,
            grant,
            token,
            expected_host: expected_host.into(),
            expected_origin,
        }
    }

    pub fn handle(&self, request: &HttpRequest) -> HttpResponse {
        self.handle_at(request, SystemTime::now())
    }

    /// Handles one request at an explicit time for deterministic callers and tests.
    pub fn handle_at(&self, request: &HttpRequest, now: SystemTime) -> HttpResponse {
        if request.method != "GET" {
            return HttpResponse::json(405, json!({"error":"method_not_allowed","read_only":true}));
        }
        if request.body.len() > MAX_HTTP_BODY_BYTES {
            return HttpResponse::json(413, json!({"error":"request_too_large"}));
        }
        if request.header("host") != Some(self.expected_host.as_str()) {
            return HttpResponse::json(400, json!({"error":"host_not_allowed"}));
        }
        if let Some(expected_origin) = &self.expected_origin
            && request.header("origin") != Some(expected_origin.as_str())
        {
            return HttpResponse::json(403, json!({"error":"origin_not_allowed"}));
        }
        if ["host", "origin", "authorization"]
            .iter()
            .any(|name| duplicate_header(request, name))
        {
            return HttpResponse::json(400, json!({"error":"duplicate_security_header"}));
        }
        let (path, query) = split_target(&request.target);
        if query.iter().any(|(key, _)| {
            key.eq_ignore_ascii_case("token") || key.eq_ignore_ascii_case("authorization")
        }) {
            return HttpResponse::json(400, json!({"error":"token_must_not_be_in_url"}));
        }
        if path == "/health" {
            return HttpResponse::json(
                200,
                json!({"status":"ok","read_only":true,"provider_calls":0,"game_launches":0}),
            );
        }
        if !self.authenticated(request) {
            return HttpResponse::json(401, json!({"error":"read_capability_required"}));
        }
        match self.route(path, &query, now) {
            Ok(response) => response,
            Err(error) => error_response(error),
        }
    }

    pub fn serve_once(&self, listener: &TcpListener) -> Result<(), ApiError> {
        let (mut stream, _) = listener.accept().map_err(|_| ApiError::Io)?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .map_err(|_| ApiError::Io)?;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 2048];
        loop {
            let read = stream.read(&mut buffer).map_err(|_| ApiError::Io)?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.len() > MAX_HTTP_REQUEST_BYTES {
                let response = error_response(ApiError::TooLarge);
                response.write_to(&mut stream)?;
                return Ok(());
            }
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let response = match HttpRequest::parse(&bytes) {
            Ok(request) => self.handle(&request),
            Err(error) => error_response(error),
        };
        response.write_to(&mut stream)
    }

    pub fn bind_loopback(port: u16) -> Result<TcpListener, ApiError> {
        TcpListener::bind(("127.0.0.1", port)).map_err(|_| ApiError::Io)
    }

    pub fn local_addr(listener: &TcpListener) -> Result<SocketAddr, ApiError> {
        listener.local_addr().map_err(|_| ApiError::Io)
    }

    fn authenticated(&self, request: &HttpRequest) -> bool {
        let Some(value) = request.header("authorization") else {
            return false;
        };
        let Some(token) = value.strip_prefix("Bearer ") else {
            return false;
        };
        token.as_bytes() == self.token
    }

    fn route(
        &self,
        path: &str,
        query: &[(String, String)],
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        if path.contains("..") || path.contains('\\') || path.contains('%') {
            return Err(ApiError::BadRequest);
        }
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments == ["v1", "capabilities"] {
            self.store
                .authorize(self.token, self.grant, now)
                .map_err(map_read_error)?;
            return Ok(HttpResponse::json(200, capabilities()));
        }
        if segments == ["v1", "runs"] {
            let limit = query_limit(query)?;
            let summaries = self
                .store
                .list(self.token, self.grant, now, limit)
                .map_err(map_read_error)?;
            let mut runs = Vec::new();
            for summary in summaries {
                if !runs.iter().any(|run: &String| run == &summary.run_id) {
                    runs.push(summary.run_id);
                }
            }
            return Ok(HttpResponse::json(
                200,
                json!({"runs":runs,"next_cursor":null}),
            ));
        }
        if segments.len() == 4
            && segments[..3] == ["v1", "runs", segments[2]]
            && segments[3] == "snapshots"
        {
            let run_id = segments[2];
            let limit = query_limit(query)?;
            let summaries = self
                .store
                .list(self.token, self.grant, now, limit)
                .map_err(map_read_error)?
                .into_iter()
                .filter(|summary| summary.run_id == run_id)
                .collect::<Vec<_>>();
            return Ok(HttpResponse::json(
                200,
                json!({"run_id":run_id,"snapshots":summaries,"next_cursor":null}),
            ));
        }
        if segments.len() == 5
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "snapshots"
        {
            let run_id = segments[2];
            let snapshot_id = segments[4];
            let bytes = self
                .store
                .get(self.token, self.grant, snapshot_id, now)
                .map_err(map_read_error)?;
            let snapshot: Value =
                serde_json::from_slice(&bytes).map_err(|_| ApiError::BadRequest)?;
            if snapshot
                .get("identity")
                .and_then(|value| value.get("run_id"))
                .and_then(Value::as_str)
                != Some(run_id)
            {
                return Err(ApiError::NotFound);
            }
            return Ok(HttpResponse::raw_json(200, bytes));
        }
        if segments.len() == 8
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "snapshots"
            && segments[5] == "components"
            && segments[7] == "content"
        {
            let run_id = segments[2];
            let snapshot_id = segments[4];
            let component_id = segments[6];
            let summary = self
                .store
                .component(self.token, self.grant, snapshot_id, component_id, now)
                .map_err(map_read_error)?;
            if !self
                .store
                .list(self.token, self.grant, now, 200)
                .map_err(map_read_error)?
                .iter()
                .any(|item| item.snapshot_id == snapshot_id && item.run_id == run_id)
            {
                return Err(ApiError::NotFound);
            }
            let bytes = self
                .store
                .content(self.token, self.grant, snapshot_id, component_id, now)
                .map_err(map_read_error)?;
            return Ok(HttpResponse::content(200, &summary.media_type, bytes));
        }
        if segments.len() == 7
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "snapshots"
            && segments[5] == "components"
        {
            let run_id = segments[2];
            let snapshot_id = segments[4];
            let component_id = segments[6];
            let summary = self
                .store
                .component(self.token, self.grant, snapshot_id, component_id, now)
                .map_err(map_read_error)?;
            if summary.snapshot_id.is_empty()
                || !self
                    .store
                    .list(self.token, self.grant, now, 200)
                    .map_err(map_read_error)?
                    .iter()
                    .any(|item| item.snapshot_id == snapshot_id && item.run_id == run_id)
            {
                return Err(ApiError::NotFound);
            }
            let content = if self.grant.privilege() == CapturePrivilege::Content {
                "content_privilege_granted"
            } else {
                "content_not_authorized"
            };
            return Ok(HttpResponse::json(
                200,
                json!({"component":summary,"content":content}),
            ));
        }
        if segments.len() == 4
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "events"
        {
            let run_id = segments[2];
            let after = query
                .iter()
                .find(|(key, _)| key == "cursor")
                .map(|(_, value)| value.parse::<u64>().map_err(|_| ApiError::BadRequest))
                .transpose()?;
            let page = self
                .store
                .events(
                    self.token,
                    self.grant,
                    run_id,
                    after,
                    query_limit(query)?,
                    now,
                )
                .map_err(map_read_error)?;
            let events: Vec<Value> = page
                .events
                .iter()
                .map(|event| {
                    json!({
                        "event_id":event.event_id,
                        "producer_id":event.producer_id,
                        "sequence":event.sequence,
                        "snapshot_id":event.snapshot_id,
                        "provider_attempt_id":event.provider_attempt_id,
                        "observed_at":event.observed_at,
                        "event_type":event.event_type.as_str(),
                        "details":event.details,
                    })
                })
                .collect();
            return Ok(HttpResponse::json(
                200,
                json!({"run_id":run_id,"events":events,"next_cursor":page.next_cursor,"gap":page.gap,"offline":false}),
            ));
        }
        if segments.len() == 4
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "compare"
        {
            let run_id = segments[2];
            let left = query_value(query, "left")?;
            let right = query_value(query, "right")?;
            let result = self
                .store
                .compare_for_run(self.token, self.grant, run_id, left, right, now)
                .map_err(map_read_error)?;
            return Ok(HttpResponse::json(
                200,
                json!({"comparison":result,"read_only":true}),
            ));
        }
        Err(ApiError::NotFound)
    }
}

fn capabilities() -> Value {
    json!({
        "schema":"ascension.context-capabilities.v1",
        "product_phase":1,
        "application_boundary_inspection":true,
        "provider_added_context":"not_exposed",
        "capture_modes":["off","metadata","memory","private"],
        "content_read_requires_approval":true,
        "context_edit":false,
        "context_compact":false,
        "pause_resume":false,
        "provider_submit":false,
        "game_dispatch":false,
        "actual_adapter_images":"unsupported"
    })
}

fn split_target(target: &str) -> (&str, Vec<(String, String)>) {
    let Some((path, query)) = target.split_once('?') else {
        return (target, Vec::new());
    };
    let values = query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (key.to_owned(), value.to_owned())
        })
        .collect();
    (path, values)
}

fn query_limit(query: &[(String, String)]) -> Result<usize, ApiError> {
    let value = query
        .iter()
        .find(|(key, _)| key == "limit")
        .map(|(_, value)| value.as_str())
        .unwrap_or("50");
    let limit = value.parse::<usize>().map_err(|_| ApiError::BadRequest)?;
    if limit == 0 || limit > 200 {
        return Err(ApiError::TooLarge);
    }
    Ok(limit)
}

fn query_value<'a>(query: &'a [(String, String)], key: &str) -> Result<&'a str, ApiError> {
    query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty() && valid_id(value))
        .ok_or(ApiError::BadRequest)
}

fn duplicate_header(request: &HttpRequest, name: &str) -> bool {
    request
        .headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .nth(1)
        .is_some()
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}

fn error_response(error: ApiError) -> HttpResponse {
    let (status, code) = match error {
        ApiError::BadRequest => (400, "bad_request"),
        ApiError::Unauthorized => (401, "unauthorized"),
        ApiError::Forbidden => (403, "forbidden"),
        ApiError::NotFound => (404, "not_found"),
        ApiError::MethodNotAllowed => (405, "method_not_allowed"),
        ApiError::TooLarge => (429, "bounded_limit"),
        ApiError::TooManyRequests => (429, "bounded_limit"),
        ApiError::Io => (500, "local_io"),
    };
    HttpResponse::json(status, json!({"error":code,"read_only":true}))
}

fn map_read_error(error: ReadError) -> ApiError {
    match error {
        ReadError::InvalidToken | ReadError::Expired => ApiError::Unauthorized,
        ReadError::InvalidScope | ReadError::Forbidden => ApiError::Forbidden,
        ReadError::NotFound => ApiError::NotFound,
        ReadError::TooLarge => ApiError::TooLarge,
    }
}

fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn ascii_token(bytes: &[u8]) -> Result<String, ApiError> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(ApiError::BadRequest);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| ApiError::BadRequest)
}

#[allow(dead_code)]
fn _epoch_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{CapturePrivilege, ReadGrant, Store};
    use std::time::Duration;

    const FIXTURE: &[u8] = include_bytes!("../../../fixtures/synthetic/snapshot.json");

    fn api() -> (Store, ReadGrant) {
        let mut store = Store::default();
        store.ingest(FIXTURE).expect("fixture");
        let grant = ReadGrant::issue(
            b"api-token",
            "agent-t02-reader",
            None,
            CapturePrivilege::Metadata,
            Duration::from_secs(60),
            UNIX_EPOCH,
        )
        .expect("grant");
        (store, grant)
    }

    #[test]
    fn health_is_minimal_and_mutation_is_rejected() {
        let (store, grant) = api();
        let api = ReadApi::new(
            &store,
            &grant,
            b"api-token",
            "127.0.0.1:0",
            Some("http://127.0.0.1:0".to_owned()),
            UNIX_EPOCH,
        );
        let health = api.handle_at(
            &HttpRequest {
                method: "GET".to_owned(),
                target: "/health".to_owned(),
                headers: vec![
                    ("host".to_owned(), "127.0.0.1:0".to_owned()),
                    ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                ],
                body: Vec::new(),
            },
            UNIX_EPOCH,
        );
        assert_eq!(health.status, 200);
        let post = api.handle_at(
            &HttpRequest {
                method: "POST".to_owned(),
                target: "/v1/capabilities".to_owned(),
                headers: vec![("host".to_owned(), "127.0.0.1:0".to_owned())],
                body: Vec::new(),
            },
            UNIX_EPOCH,
        );
        assert_eq!(post.status, 405);
    }

    #[test]
    fn url_tokens_and_wrong_origins_are_rejected() {
        let (store, grant) = api();
        let api = ReadApi::new(
            &store,
            &grant,
            b"api-token",
            "127.0.0.1:0",
            Some("http://127.0.0.1:0".to_owned()),
            UNIX_EPOCH,
        );
        let response = api.handle_at(
            &HttpRequest {
                method: "GET".to_owned(),
                target: "/v1/capabilities?token=api-token".to_owned(),
                headers: vec![
                    ("host".to_owned(), "127.0.0.1:0".to_owned()),
                    ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                    ("authorization".to_owned(), "Bearer api-token".to_owned()),
                ],
                body: Vec::new(),
            },
            UNIX_EPOCH,
        );
        assert_eq!(response.status, 400);
        let response = api.handle_at(
            &HttpRequest {
                method: "GET".to_owned(),
                target: "/v1/capabilities".to_owned(),
                headers: vec![
                    ("host".to_owned(), "127.0.0.1:0".to_owned()),
                    ("origin".to_owned(), "https://evil.invalid".to_owned()),
                    ("authorization".to_owned(), "Bearer api-token".to_owned()),
                ],
                body: Vec::new(),
            },
            UNIX_EPOCH,
        );
        assert_eq!(response.status, 403);
    }

    #[test]
    fn reused_api_checks_capability_expiry_at_each_request() {
        let (store, grant) = api();
        let api = ReadApi::new(
            &store,
            &grant,
            b"api-token",
            "127.0.0.1:0",
            Some("http://127.0.0.1:0".to_owned()),
            UNIX_EPOCH,
        );
        let request = |target: &str| HttpRequest {
            method: "GET".to_owned(),
            target: target.to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                ("authorization".to_owned(), "Bearer api-token".to_owned()),
            ],
            body: Vec::new(),
        };
        assert_eq!(
            api.handle_at(&request("/v1/capabilities"), UNIX_EPOCH)
                .status,
            200
        );
        assert_eq!(
            api.handle_at(
                &request("/v1/capabilities"),
                UNIX_EPOCH + Duration::from_secs(60),
            )
            .status,
            401
        );
    }

    #[test]
    fn capabilities_honor_revocation() {
        let (mut store, grant) = api();
        store.revoke(b"api-token").expect("revoke");
        let api = ReadApi::new(
            &store,
            &grant,
            b"api-token",
            "127.0.0.1:0",
            Some("http://127.0.0.1:0".to_owned()),
            UNIX_EPOCH,
        );
        let response = api.handle_at(
            &HttpRequest {
                method: "GET".to_owned(),
                target: "/v1/capabilities".to_owned(),
                headers: vec![
                    ("host".to_owned(), "127.0.0.1:0".to_owned()),
                    ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                    ("authorization".to_owned(), "Bearer api-token".to_owned()),
                ],
                body: Vec::new(),
            },
            UNIX_EPOCH,
        );
        assert_eq!(response.status, 403);
    }
}
