// SPDX-License-Identifier: MIT

//! Small loopback read surface.  It parses a bounded HTTP request through `crate::http`,
//! authenticates a bearer capability from the header, and dispatches only GET resources backed by
//! the immutable store. There is no process, provider, game, URL-fetch or mutation path here.

mod api;
mod auth;
mod capabilities;
mod handlers;
mod router;
#[cfg(test)]
mod tests;

pub use crate::http::{
    ApiError, HttpRequest, HttpResponse, MAX_HTTP_BODY_BYTES, MAX_HTTP_REQUEST_BYTES,
};
pub use api::ReadApi;

pub(crate) use auth::valid_id;
