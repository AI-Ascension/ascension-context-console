use crate::http::{ApiError, HttpRequest};
use serde::Serialize;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

pub(super) const MAX_HEADER_BYTES: usize = 8 * 1024;
pub(super) const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_REQUEST_BYTES: usize = MAX_HEADER_BYTES + MAX_BODY_BYTES;
const IO_SLICE_BYTES: usize = 4096;

pub(super) struct SensitiveRequest(pub(super) HttpRequest);

impl Drop for SensitiveRequest {
    fn drop(&mut self) {
        self.0.method.zeroize();
        self.0.target.zeroize();
        for (name, value) in &mut self.0.headers {
            name.zeroize();
            value.zeroize();
        }
        self.0.body.zeroize();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReadError {
    BadRequest,
    TooLarge,
    Deadline,
    Io,
}

pub(super) fn read_request(
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<SensitiveRequest, ReadError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(4096));
    let mut buffer = Zeroizing::new([0_u8; IO_SLICE_BYTES]);
    let header_end = loop {
        let read = read_slice(stream, &mut buffer[..], deadline)?;
        if read == 0 {
            return Err(ReadError::BadRequest);
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            if index + 4 > MAX_HEADER_BYTES {
                return Err(ReadError::TooLarge);
            }
            break index + 4;
        }
        if bytes.len() > MAX_HEADER_BYTES {
            return Err(ReadError::TooLarge);
        }
    };
    let head = SensitiveRequest(
        HttpRequest::parse_with_limits(
            &bytes[..header_end],
            false,
            MAX_REQUEST_BYTES,
            MAX_BODY_BYTES,
            MAX_HEADER_BYTES,
        )
        .map_err(map_parse_error)?,
    );
    let body_length =
        crate::http::declared_content_length(&head.0.headers).map_err(|error| match error {
            ApiError::TooLarge => ReadError::TooLarge,
            _ => ReadError::BadRequest,
        })?;
    let total = header_end
        .checked_add(body_length)
        .filter(|size| *size <= MAX_REQUEST_BYTES)
        .ok_or(ReadError::TooLarge)?;
    if body_length > MAX_BODY_BYTES || bytes.len() > total {
        return Err(if body_length > MAX_BODY_BYTES {
            ReadError::TooLarge
        } else {
            ReadError::BadRequest
        });
    }
    while bytes.len() < total {
        let read = read_slice(stream, &mut buffer[..], deadline)?;
        if read == 0 || bytes.len().saturating_add(read) > total {
            return Err(ReadError::BadRequest);
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    HttpRequest::parse_with_limits(
        &bytes,
        true,
        MAX_REQUEST_BYTES,
        MAX_BODY_BYTES,
        MAX_HEADER_BYTES,
    )
    .map(SensitiveRequest)
    .map_err(map_parse_error)
}

fn read_slice(
    stream: &mut TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
) -> Result<usize, ReadError> {
    set_timeout(stream, deadline, false)?;
    stream.read(buffer).map_err(map_io_error)
}

pub(super) fn serialize_bounded<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, ApiError> {
    let mut writer = BoundedJsonWriter(Zeroizing::new(Vec::with_capacity(4096)));
    serde_json::to_writer(&mut writer, value).map_err(|_| ApiError::TooLarge)?;
    Ok(writer.0)
}

struct BoundedJsonWriter(Zeroizing<Vec<u8>>);

impl Write for BoundedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let size = self
            .0
            .len()
            .checked_add(buffer.len())
            .ok_or_else(|| io::Error::new(io::ErrorKind::WriteZero, "bounded response"))?;
        if size > MAX_BODY_BYTES {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "bounded response"));
        }
        self.0
            .try_reserve_exact(buffer.len())
            .map_err(|_| io::Error::other("bounded response"))?;
        self.0.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn write_response(
    stream: &mut TcpStream,
    status: u16,
    body: &[u8],
    deadline: Instant,
) -> Result<(), ReadError> {
    if body.len() > MAX_BODY_BYTES {
        return Err(ReadError::TooLarge);
    }
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Content",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => return Err(ReadError::Io),
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    write_slice(stream, header.as_bytes(), deadline)?;
    write_slice(stream, body, deadline)
}

fn write_slice(stream: &TcpStream, bytes: &[u8], deadline: Instant) -> Result<(), ReadError> {
    write_all_with_deadline(
        bytes,
        deadline,
        Instant::now,
        |remaining| {
            stream
                .set_write_timeout(Some(remaining))
                .map_err(|_| ReadError::Io)
        },
        |remaining| {
            let mut writer = stream;
            writer.write(remaining)
        },
    )
}

pub(super) fn write_all_with_deadline(
    bytes: &[u8],
    deadline: Instant,
    mut now: impl FnMut() -> Instant,
    mut set_write_timeout: impl FnMut(Duration) -> Result<(), ReadError>,
    mut write: impl FnMut(&[u8]) -> io::Result<usize>,
) -> Result<(), ReadError> {
    let mut written = 0;
    if now() >= deadline {
        return Err(ReadError::Deadline);
    }
    while written < bytes.len() {
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() {
            return Err(ReadError::Deadline);
        }
        set_write_timeout(remaining)?;
        if now() >= deadline {
            return Err(ReadError::Deadline);
        }
        match write(&bytes[written..]) {
            Ok(0) => return Err(ReadError::Io),
            Ok(count) if count <= bytes.len() - written => {
                written += count;
                if now() >= deadline {
                    return Err(ReadError::Deadline);
                }
            }
            Ok(_) => return Err(ReadError::Io),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(map_io_error(error)),
        }
    }
    if now() >= deadline {
        Err(ReadError::Deadline)
    } else {
        Ok(())
    }
}

fn set_timeout(stream: &TcpStream, deadline: Instant, writing: bool) -> Result<(), ReadError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ReadError::Deadline);
    }
    let timeout = Some(remaining);
    if writing {
        stream.set_write_timeout(timeout)
    } else {
        stream.set_read_timeout(timeout)
    }
    .map_err(|_| ReadError::Io)
}

fn map_io_error(error: io::Error) -> ReadError {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => ReadError::Deadline,
        io::ErrorKind::InvalidData => ReadError::BadRequest,
        _ => ReadError::Io,
    }
}

fn map_parse_error(error: ApiError) -> ReadError {
    match error {
        ApiError::TooLarge | ApiError::TooManyRequests => ReadError::TooLarge,
        _ => ReadError::BadRequest,
    }
}
