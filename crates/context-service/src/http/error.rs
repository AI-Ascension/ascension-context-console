// SPDX-License-Identifier: MIT

use super::response::HttpResponse;
use serde_json::json;

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

pub(crate) fn error_response(error: ApiError) -> HttpResponse {
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
