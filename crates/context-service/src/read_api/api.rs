// SPDX-License-Identifier: MIT

use crate::http::{
    ApiError, HttpRequest, HttpResponse, declared_content_length, error_response,
    read_request_bytes, split_target,
};
use crate::store::{ReadGrant, Store};
use serde_json::json;
use std::net::{SocketAddr, TcpListener};
use std::time::SystemTime;

pub struct ReadApi<'a> {
    pub(super) store: &'a Store,
    pub(super) grant: &'a ReadGrant,
    pub(super) token: &'a [u8],
    pub(super) expected_host: String,
    pub(super) expected_origin: Option<String>,
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
        match declared_content_length(&request.headers) {
            Ok(length) if length == request.body.len() => {}
            Ok(_) | Err(ApiError::BadRequest) => {
                return HttpResponse::json(400, json!({"error":"invalid_request_framing"}));
            }
            Err(error) => return error_response(error),
        }
        if request.method != "GET" {
            return HttpResponse::json(405, json!({"error":"method_not_allowed","read_only":true}));
        }
        if !request.body.is_empty() {
            return HttpResponse::json(
                400,
                json!({"error":"get_body_not_allowed","read_only":true}),
            );
        }
        if let Some(response) = self.security_rejection(request) {
            return response;
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
        let bytes = match read_request_bytes(&mut stream) {
            Ok(bytes) => bytes,
            Err(error) => {
                error_response(error).write_to(&mut stream)?;
                return Ok(());
            }
        };
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
}
