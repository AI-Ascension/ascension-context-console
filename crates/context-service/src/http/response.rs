// SPDX-License-Identifier: MIT

use super::error::ApiError;
use serde_json::Value;
use std::io::Write;
use std::net::TcpStream;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub(crate) fn json(status: u16, value: Value) -> Self {
        let body =
            serde_json::to_vec(&value).unwrap_or_else(|_| b"{\"error\":\"encoding\"}".to_vec());
        Self {
            status,
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Cache-Control".to_owned(), "no-store".to_owned()),
                ("Content-Length".to_owned(), body.len().to_string()),
                ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
            ],
            body,
        }
    }

    pub(crate) fn raw_json(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Cache-Control".to_owned(), "no-store".to_owned()),
                ("Content-Length".to_owned(), body.len().to_string()),
                ("X-Content-Type-Options".to_owned(), "nosniff".to_owned()),
            ],
            body,
        }
    }

    pub(crate) fn content(status: u16, media_type: &str, body: Vec<u8>) -> Self {
        Self {
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

    pub fn write_to(&self, stream: &mut TcpStream) -> Result<(), ApiError> {
        let reason = match self.status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            429 => "Too Many Requests",
            _ => "Internal Server Error",
        };
        write!(stream, "HTTP/1.1 {} {}\r\n", self.status, reason).map_err(|_| ApiError::Io)?;
        for (name, value) in &self.headers {
            write!(stream, "{name}: {value}\r\n").map_err(|_| ApiError::Io)?;
        }
        stream
            .write_all(b"Connection: close\r\n\r\n")
            .map_err(|_| ApiError::Io)?;
        stream.write_all(&self.body).map_err(|_| ApiError::Io)
    }
}
