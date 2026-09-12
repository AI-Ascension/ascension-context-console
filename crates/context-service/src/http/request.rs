// SPDX-License-Identifier: MIT

use super::error::ApiError;
use super::framing::{ascii_token, trim_cr};

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
        Self::parse_with_body_framing(bytes, true)
    }

    pub(crate) fn parse_head(bytes: &[u8]) -> Result<Self, ApiError> {
        Self::parse_with_body_framing(bytes, false)
    }

    pub(crate) fn parse_with_body_framing(
        bytes: &[u8],
        enforce_declared_body_length: bool,
    ) -> Result<Self, ApiError> {
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
        let declared_body_length = declared_content_length(&headers)?;
        if enforce_declared_body_length && body.len() != declared_body_length {
            return Err(ApiError::BadRequest);
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

pub(crate) fn declared_content_length(headers: &[(String, String)]) -> Result<usize, ApiError> {
    let mut content_length = None;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(ApiError::BadRequest);
        }
        if !name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        if content_length.is_some() {
            return Err(ApiError::BadRequest);
        }
        let length = value.parse::<usize>().map_err(|_| ApiError::BadRequest)?;
        if length > MAX_HTTP_BODY_BYTES {
            return Err(ApiError::TooLarge);
        }
        content_length = Some(length);
    }
    Ok(content_length.unwrap_or(0))
}
