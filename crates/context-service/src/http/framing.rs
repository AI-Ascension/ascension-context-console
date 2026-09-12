// SPDX-License-Identifier: MIT

use super::error::ApiError;
use super::request::{HttpRequest, MAX_HTTP_REQUEST_BYTES, declared_content_length};
use std::io::{ErrorKind, Read};
use std::net::TcpStream;

pub(crate) fn read_request_bytes(stream: &mut TcpStream) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 2048];
    let header_end = loop {
        let read = read_from_stream(stream, &mut buffer)?;
        if read == 0 {
            return Err(ApiError::BadRequest);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(ApiError::TooLarge);
        }
        if let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break split + 4;
        }
    };

    let head = HttpRequest::parse_head(&bytes[..header_end])?;
    let body_length = declared_content_length(&head.headers)?;
    let total_length = header_end
        .checked_add(body_length)
        .ok_or(ApiError::TooLarge)?;
    if total_length > MAX_HTTP_REQUEST_BYTES || bytes.len() > total_length {
        return Err(if total_length > MAX_HTTP_REQUEST_BYTES {
            ApiError::TooLarge
        } else {
            ApiError::BadRequest
        });
    }
    while bytes.len() < total_length {
        let read = read_from_stream(stream, &mut buffer)?;
        if read == 0 {
            return Err(ApiError::BadRequest);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > total_length || bytes.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(ApiError::BadRequest);
        }
    }
    Ok(bytes)
}

pub(crate) fn read_from_stream(
    stream: &mut TcpStream,
    buffer: &mut [u8],
) -> Result<usize, ApiError> {
    stream.read(buffer).map_err(|error| {
        if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) {
            ApiError::BadRequest
        } else {
            ApiError::Io
        }
    })
}

pub(crate) fn trim_cr(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\r").unwrap_or(line)
}

pub(crate) fn ascii_token(bytes: &[u8]) -> Result<String, ApiError> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(ApiError::BadRequest);
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| ApiError::BadRequest)
}
