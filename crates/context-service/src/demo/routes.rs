// SPDX-License-Identifier: MIT

//! Same-origin routing for the loopback demonstration server.
//!
//! `/demo/*` adapts browser requests to the authenticated `ReadApi`; `/web/*` and
//! `/offline-bundle.json` serve the checked-in static review surface. Every response is
//! read-only and served from local bytes.

use super::fixtures;
use super::parse_json;
use super::state::DemoState;
use super::{
    COMPARISON_ID, CSRF_TOKEN, EDITOR_TOKEN, OBJECTIVE_TOKEN, RUN, SESSION_PRINCIPAL,
    SESSION_TOKEN, SNAPSHOT_ID,
};
use crate::control::{Command, ControlError, Patch, Scope};
use crate::http::{HttpRequest, HttpResponse, split_target};
use crate::memory::MemoryRouteError;
use crate::owner::OwnerRequestContext;
use crate::provider_session::SessionApiError;
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
            "/web/js/policy-owner.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                fixtures::WEB_POLICY_OWNER.to_vec(),
            ),
            "/web/js/context-owner.js" => static_response(
                200,
                "text/javascript; charset=utf-8",
                fixtures::WEB_CONTEXT_OWNER.to_vec(),
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
        let result = if self.memory.is_attached() {
            let context = OwnerRequestContext::new(
                token,
                request
                    .header("host")
                    .map_or_else(String::new, ToOwned::to_owned),
                request.header("origin").map(ToOwned::to_owned),
                request.header("x-csrf-token").map(ToOwned::to_owned),
                unix_seconds(),
            );
            self.memory
                .handle_with_context(&request.method, path, &context, &request.body)
        } else {
            self.memory
                .handle(&request.method, path, token, &request.body)
        };
        match result {
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
        let result = if self.provider_sessions.is_attached() {
            let context = OwnerRequestContext::new(
                SESSION_PRINCIPAL,
                request
                    .header("host")
                    .map_or_else(String::new, ToOwned::to_owned),
                request.header("origin").map(ToOwned::to_owned),
                request.header("x-csrf-token").map(ToOwned::to_owned),
                unix_seconds(),
            );
            self.provider_sessions.handle_with_context(
                &request.method,
                path,
                &context,
                &request.body,
            )
        } else {
            self.provider_sessions
                .handle(&request.method, path, SESSION_PRINCIPAL, &request.body)
        };
        match result {
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
        MemoryRouteError::Unavailable => 503,
        MemoryRouteError::EffectiveLimit(_) => 422,
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
        SessionApiError::EffectiveLimit(_) => 422,
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

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
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
