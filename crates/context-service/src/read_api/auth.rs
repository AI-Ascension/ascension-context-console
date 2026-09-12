// SPDX-License-Identifier: MIT

use super::ReadApi;
use super::handlers::map_read_error;
use crate::http::{ApiError, HttpRequest, HttpResponse};
use serde_json::json;
use std::time::SystemTime;

impl<'a> ReadApi<'a> {
    /// Rejects requests that violate the loopback host/origin and header rules before dispatch.
    pub(super) fn security_rejection(&self, request: &HttpRequest) -> Option<HttpResponse> {
        // Query keys are deliberately not percent-decoded. Rejecting escapes across the complete
        // target prevents an encoded credential key such as `%74oken` from bypassing the URL rule.
        if request.target.contains('%') {
            return Some(HttpResponse::json(
                400,
                json!({"error":"percent_escape_not_allowed"}),
            ));
        }
        if request.header("host") != Some(self.expected_host.as_str()) {
            return Some(HttpResponse::json(400, json!({"error":"host_not_allowed"})));
        }
        if let Some(expected_origin) = &self.expected_origin
            && request.header("origin") != Some(expected_origin.as_str())
        {
            return Some(HttpResponse::json(
                403,
                json!({"error":"origin_not_allowed"}),
            ));
        }
        if ["host", "origin", "authorization"]
            .iter()
            .any(|name| duplicate_header(request, name))
        {
            return Some(HttpResponse::json(
                400,
                json!({"error":"duplicate_security_header"}),
            ));
        }
        None
    }

    pub(super) fn authenticated(&self, request: &HttpRequest) -> bool {
        let Some(value) = request.header("authorization") else {
            return false;
        };
        let Some(token) = value.strip_prefix("Bearer ") else {
            return false;
        };
        token.as_bytes() == self.token
    }

    /// Checks revocation and expiry of the presented capability against the store.
    pub(super) fn authorize(&self, now: SystemTime) -> Result<(), ApiError> {
        self.store
            .authorize(self.token, self.grant, now)
            .map_err(map_read_error)
    }
}

pub(super) fn duplicate_header(request: &HttpRequest, name: &str) -> bool {
    request
        .headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .nth(1)
        .is_some()
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}
