use super::{
    HarnessOwnerTransportConfig, MAX_BEARER_BYTES, MAX_HARNESS_JSON_BODY_BYTES, MAX_PATH_BYTES,
};
use crate::harness_context_owner_wire::{ContextOwnerEndpointV1, HarnessHttpMethod};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MAX_HTTP_HEADER_BYTES: usize = 8 * 1024;
const HTTP_READ_CHUNK_BYTES: usize = 1024;

pub(super) struct HttpReply {
    pub status: u16,
    pub body: Zeroizing<Vec<u8>>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum FrameError {
    Deadline,
    Io,
    InvalidFraming,
    ResponseTooLarge,
}

#[cfg(test)]
pub(super) fn exchange(
    config: &HarnessOwnerTransportConfig,
    endpoint: &ContextOwnerEndpointV1,
    body: Option<&[u8]>,
    bearer: &[u8],
    request_deadline: Instant,
) -> Result<HttpReply, FrameError> {
    let request_deadline = operation_deadline(config.deadline, request_deadline)?;
    exchange_with_authorization(config, endpoint, body, bearer, request_deadline, || {
        Ok(request_deadline)
    })
}

pub(super) fn operation_deadline(
    configured_deadline: Duration,
    request_deadline: Instant,
) -> Result<Instant, FrameError> {
    let configured_cap = Instant::now()
        .checked_add(configured_deadline)
        .ok_or(FrameError::Deadline)?;
    Ok(configured_cap.min(request_deadline))
}

pub(super) fn exchange_with_authorization<E, F>(
    config: &HarnessOwnerTransportConfig,
    endpoint: &ContextOwnerEndpointV1,
    body: Option<&[u8]>,
    bearer: &[u8],
    request_deadline: Instant,
    authorize_before_connect: F,
) -> Result<HttpReply, E>
where
    E: From<FrameError>,
    F: FnOnce() -> Result<Instant, E>,
{
    if bearer.is_empty()
        || bearer.len() > MAX_BEARER_BYTES
        || bearer.iter().any(|byte| !(0x21..=0x7e).contains(byte))
        || body.is_some_and(|bytes| bytes.len() > MAX_HARNESS_JSON_BODY_BYTES)
        || (endpoint.method == HarnessHttpMethod::Get) != body.is_none()
    {
        return Err(FrameError::InvalidFraming.into());
    }
    let target = request_target(endpoint).map_err(E::from)?;
    let request = build_request(config, endpoint.method, &target, body, bearer).map_err(E::from)?;
    // `request_deadline` already includes the configured cap captured at operation entry. Do
    // not restart that budget here after admission, redemption, or store work.
    let request_cap = request_deadline;
    remaining(request_cap).map_err(E::from)?;
    // Run required live-currentness validation after request assembly and immediately before
    // opening the socket. This closes the gap between admission and a potentially slow frame.
    let admission_deadline = authorize_before_connect()?;
    let deadline = request_cap.min(admission_deadline);
    let timeout = remaining(deadline).map_err(E::from)?;
    let mut stream = TcpStream::connect_timeout(&config.address, timeout)
        .map_err(map_io)
        .map_err(E::from)?;
    write_with_deadline(&mut stream, &request, deadline).map_err(E::from)?;
    read_response(&mut stream, deadline).map_err(E::from)
}

fn request_target(endpoint: &ContextOwnerEndpointV1) -> Result<String, FrameError> {
    if endpoint.path.len() > MAX_PATH_BYTES
        || !endpoint.path.starts_with("/v1/workflow-runs/")
        || !endpoint.path.is_ascii()
        || endpoint
            .path
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"/._:-".contains(&byte)))
        || endpoint.query.len() > 8
    {
        return Err(FrameError::InvalidFraming);
    }
    let mut target = endpoint.path.clone();
    let mut seen = std::collections::BTreeSet::new();
    for (index, query) in endpoint.query.iter().enumerate() {
        if !matches!(
            query.name.as_str(),
            "draft_id" | "include_content" | "after_revision_id" | "limit"
        ) || query.name.is_empty()
            || query.value.is_empty()
            || query.name.len() > 128
            || query.value.len() > 128
            || !safe_query_component(&query.name)
            || !safe_query_component(&query.value)
            || !seen.insert(query.name.as_str())
        {
            return Err(FrameError::InvalidFraming);
        }
        target.push(if index == 0 { '?' } else { '&' });
        target.push_str(&query.name);
        target.push('=');
        target.push_str(&query.value);
    }
    if target.len() > MAX_PATH_BYTES {
        return Err(FrameError::InvalidFraming);
    }
    Ok(target)
}

fn safe_query_component(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn build_request(
    config: &HarnessOwnerTransportConfig,
    method: HarnessHttpMethod,
    target: &str,
    body: Option<&[u8]>,
    bearer: &[u8],
) -> Result<Zeroizing<Vec<u8>>, FrameError> {
    let content_length = body.map_or(0, <[u8]>::len).to_string();
    let host = config.address.to_string();
    let mut request = Zeroizing::new(Vec::with_capacity(
        target.len() + host.len() + bearer.len() + body.map_or(0, <[u8]>::len) + 256,
    ));
    request.extend_from_slice(method.as_str().as_bytes());
    request.extend_from_slice(b" ");
    request.extend_from_slice(target.as_bytes());
    request.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    request.extend_from_slice(host.as_bytes());
    request.extend_from_slice(b"\r\nAuthorization: Bearer ");
    request.extend_from_slice(bearer);
    request.extend_from_slice(b"\r\nContent-Length: ");
    request.extend_from_slice(content_length.as_bytes());
    request.extend_from_slice(b"\r\n");
    if body.is_some() {
        request.extend_from_slice(b"Content-Type: application/json\r\n");
    }
    request.extend_from_slice(b"Connection: close\r\n\r\n");
    if request.len() > MAX_HTTP_HEADER_BYTES {
        return Err(FrameError::InvalidFraming);
    }
    if let Some(body) = body {
        request.extend_from_slice(body);
    }
    Ok(request)
}

fn read_response(stream: &mut TcpStream, deadline: Instant) -> Result<HttpReply, FrameError> {
    let mut received = Zeroizing::new(Vec::with_capacity(2048));
    let header_end = loop {
        if let Some(index) = find_header_end(&received) {
            if index + 4 > MAX_HTTP_HEADER_BYTES {
                return Err(FrameError::ResponseTooLarge);
            }
            break index;
        }
        if received.len() >= MAX_HTTP_HEADER_BYTES {
            return Err(FrameError::ResponseTooLarge);
        }
        let mut buffer = Zeroizing::new([0_u8; HTTP_READ_CHUNK_BYTES]);
        let size = read_with_deadline(stream, &mut buffer[..], deadline)?;
        if size == 0 {
            return Err(FrameError::InvalidFraming);
        }
        received.extend_from_slice(&buffer[..size]);
    };
    let header_bytes = &received[..header_end];
    let (status, content_length) = parse_head(header_bytes)?;
    if content_length > MAX_HARNESS_JSON_BODY_BYTES {
        return Err(FrameError::ResponseTooLarge);
    }
    let body_start = header_end + 4;
    let already_read = &received[body_start..];
    if already_read.len() > content_length {
        return Err(FrameError::InvalidFraming);
    }
    let mut body = Zeroizing::new(Vec::with_capacity(content_length));
    body.extend_from_slice(already_read);
    while body.len() < content_length {
        let remaining_body = content_length - body.len();
        let mut buffer = Zeroizing::new([0_u8; HTTP_READ_CHUNK_BYTES]);
        let size = read_with_deadline(
            stream,
            &mut buffer[..remaining_body.min(HTTP_READ_CHUNK_BYTES)],
            deadline,
        )?;
        if size == 0 {
            return Err(FrameError::InvalidFraming);
        }
        body.extend_from_slice(&buffer[..size]);
    }
    let mut trailing = Zeroizing::new([0_u8; 1]);
    if read_with_deadline(stream, &mut trailing[..], deadline)? != 0 {
        return Err(FrameError::InvalidFraming);
    }
    Ok(HttpReply { status, body })
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn parse_head(bytes: &[u8]) -> Result<(u16, usize), FrameError> {
    if bytes.iter().any(|byte| !byte.is_ascii()) {
        return Err(FrameError::InvalidFraming);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| FrameError::InvalidFraming)?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or(FrameError::InvalidFraming)?;
    let status = parse_status_line(status_line)?;
    let mut content_length = None;
    let mut content_type = false;
    let mut connection_close = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            return Err(FrameError::InvalidFraming);
        }
        let (name, raw_value) = line.split_once(':').ok_or(FrameError::InvalidFraming)?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || raw_value.contains(['\t', '\r', '\n'])
        {
            return Err(FrameError::InvalidFraming);
        }
        let value = raw_value.trim_matches(' ');
        match name.to_ascii_lowercase().as_str() {
            "content-length" if content_length.is_none() => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(FrameError::InvalidFraming);
                }
                content_length = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| FrameError::InvalidFraming)?,
                );
            }
            "content-type" if !content_type && value.eq_ignore_ascii_case("application/json") => {
                content_type = true;
            }
            "connection" if !connection_close && value.eq_ignore_ascii_case("close") => {
                connection_close = true;
            }
            // No transfer coding, compression, proxy metadata, duplicate, or extension headers.
            _ => return Err(FrameError::InvalidFraming),
        }
    }
    let length = content_length.ok_or(FrameError::InvalidFraming)?;
    if !content_type || !connection_close {
        return Err(FrameError::InvalidFraming);
    }
    Ok((status, length))
}

fn parse_status_line(line: &str) -> Result<u16, FrameError> {
    let bytes = line.as_bytes();
    if bytes.len() < 13
        || !line.starts_with("HTTP/1.1 ")
        || !bytes[9..12].iter().all(u8::is_ascii_digit)
        || bytes[12] != b' '
        || bytes[13..].iter().any(|byte| !(0x20..=0x7e).contains(byte))
    {
        return Err(FrameError::InvalidFraming);
    }
    let status = line[9..12]
        .parse::<u16>()
        .map_err(|_| FrameError::InvalidFraming)?;
    if !(200..=599).contains(&status) {
        return Err(FrameError::InvalidFraming);
    }
    Ok(status)
}

fn write_with_deadline(
    stream: &mut TcpStream,
    bytes: &[u8],
    deadline: Instant,
) -> Result<(), FrameError> {
    let mut written = 0;
    while written < bytes.len() {
        stream
            .set_write_timeout(Some(remaining(deadline)?))
            .map_err(map_io)?;
        match stream.write(&bytes[written..]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(FrameError::Deadline);
            }
            Err(_) => return Err(FrameError::Io),
            Ok(0) => return Err(FrameError::Io),
            Ok(size) => written += size,
        }
    }
    Ok(())
}

fn read_with_deadline(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, FrameError> {
    loop {
        stream
            .set_read_timeout(Some(remaining(deadline)?))
            .map_err(map_io)?;
        match stream.read(buffer) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(FrameError::Deadline);
            }
            Err(_) => return Err(FrameError::Io),
            Ok(size) => return Ok(size),
        }
    }
}

fn remaining(deadline: Instant) -> Result<Duration, FrameError> {
    let duration = deadline.saturating_duration_since(Instant::now());
    if duration.is_zero() {
        Err(FrameError::Deadline)
    } else {
        Ok(duration)
    }
}

fn map_io(error: std::io::Error) -> FrameError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => FrameError::Deadline,
        _ => FrameError::Io,
    }
}
