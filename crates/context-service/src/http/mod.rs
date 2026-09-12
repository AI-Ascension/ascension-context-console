// SPDX-License-Identifier: MIT

//! Bounded loopback HTTP framing shared by the read API and the integrated demo.

mod error;
mod framing;
mod request;
mod response;
mod target;
#[cfg(test)]
mod tests;

pub use error::ApiError;
pub use request::{HttpRequest, MAX_HTTP_BODY_BYTES, MAX_HTTP_REQUEST_BYTES};
pub use response::HttpResponse;

pub(crate) use error::error_response;
pub(crate) use framing::read_request_bytes;
pub(crate) use request::declared_content_length;
pub(crate) use target::{query_limit, query_value, split_target};

#[allow(dead_code)]
fn _epoch_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
