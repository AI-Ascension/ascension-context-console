// SPDX-License-Identifier: MIT

use super::framing::{ascii_token, trim_cr};
use super::*;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::Duration;

fn request(method: &str, target: &str, extra_headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("{method} {target} HTTP/1.1\r\n{extra_headers}\r\n").into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

#[test]
fn parse_accepts_a_framed_request_and_lowercases_headers() {
    let bytes = request(
        "GET",
        "/v1/runs?limit=2",
        "Host: 127.0.0.1:0\r\nX-Probe: value \r\n",
        b"",
    );
    let parsed = HttpRequest::parse(&bytes).expect("parse");
    assert_eq!(parsed.method, "GET");
    assert_eq!(parsed.target, "/v1/runs?limit=2");
    assert_eq!(parsed.header("host"), Some("127.0.0.1:0"));
    assert_eq!(parsed.header("x-probe"), Some("value"));
    assert!(parsed.body.is_empty());
}

#[test]
fn parse_enforces_the_declared_body_length() {
    let bytes = request(
        "POST",
        "/ingest",
        "Host: 127.0.0.1:0\r\nContent-Length: 4\r\n",
        b"body",
    );
    let parsed = HttpRequest::parse(&bytes).expect("parse");
    assert_eq!(parsed.body, b"body");

    let truncated = request(
        "POST",
        "/ingest",
        "Host: 127.0.0.1:0\r\nContent-Length: 8\r\n",
        b"body",
    );
    assert_eq!(HttpRequest::parse(&truncated), Err(ApiError::BadRequest));

    let trailing = request(
        "POST",
        "/ingest",
        "Host: 127.0.0.1:0\r\nContent-Length: 1\r\n",
        b"body",
    );
    assert_eq!(HttpRequest::parse(&trailing), Err(ApiError::BadRequest));
}

#[test]
fn parse_head_skips_declared_body_enforcement() {
    let bytes = request(
        "POST",
        "/ingest",
        "Host: 127.0.0.1:0\r\nContent-Length: 8\r\n",
        b"",
    );
    let parsed = HttpRequest::parse_head(&bytes).expect("parse head");
    assert!(parsed.body.is_empty());
}

#[test]
fn declared_content_length_rejects_duplicates_encoding_and_oversize() {
    let headers = |value: &str| vec![("content-length".to_owned(), value.to_owned())];
    assert_eq!(declared_content_length(&[]), Ok(0));
    assert_eq!(declared_content_length(&headers("12")), Ok(12));
    assert_eq!(
        declared_content_length(&headers("not-a-number")),
        Err(ApiError::BadRequest)
    );
    assert_eq!(
        declared_content_length(&[
            ("content-length".to_owned(), "1".to_owned()),
            ("Content-Length".to_owned(), "1".to_owned()),
        ]),
        Err(ApiError::BadRequest)
    );
    assert_eq!(
        declared_content_length(&[("transfer-encoding".to_owned(), "chunked".to_owned())]),
        Err(ApiError::BadRequest)
    );
    assert_eq!(
        declared_content_length(&headers(&(MAX_HTTP_BODY_BYTES + 1).to_string())),
        Err(ApiError::TooLarge)
    );
}

#[test]
fn parse_rejects_malformed_or_unbounded_requests() {
    assert_eq!(HttpRequest::parse(b""), Err(ApiError::BadRequest));
    assert_eq!(
        HttpRequest::parse(b"GET /health HTTP/1.0\r\nHost: x\r\n\r\n"),
        Err(ApiError::BadRequest)
    );
    assert_eq!(
        HttpRequest::parse(b"GET /health HTTP/1.1\r\nHost x\r\n\r\n"),
        Err(ApiError::BadRequest)
    );
    assert_eq!(
        HttpRequest::parse(b"GET /health HTTP/1.1\nHost: x\n\n"),
        Err(ApiError::BadRequest)
    );
    let mut oversized = vec![b'G'; MAX_HTTP_REQUEST_BYTES + 1];
    oversized.extend_from_slice(b"  /health HTTP/1.1\r\n\r\n");
    assert_eq!(HttpRequest::parse(&oversized), Err(ApiError::BadRequest));
}

#[test]
fn split_target_separates_path_and_query_pairs() {
    assert_eq!(split_target("/health"), ("/health", Vec::new()));
    assert_eq!(
        split_target("/v1/runs?limit=2&cursor=abc&empty="),
        (
            "/v1/runs",
            vec![
                ("limit".to_owned(), "2".to_owned()),
                ("cursor".to_owned(), "abc".to_owned()),
                ("empty".to_owned(), String::new()),
            ]
        )
    );
    assert_eq!(
        split_target("/v1/runs?flag"),
        ("/v1/runs", vec![("flag".to_owned(), String::new())])
    );
}

#[test]
fn query_limit_is_bounded_and_defaulted() {
    assert_eq!(query_limit(&[]), Ok(50));
    assert_eq!(
        query_limit(&[("limit".to_owned(), "200".to_owned())]),
        Ok(200)
    );
    assert_eq!(
        query_limit(&[("limit".to_owned(), "0".to_owned())]),
        Err(ApiError::TooLarge)
    );
    assert_eq!(
        query_limit(&[("limit".to_owned(), "201".to_owned())]),
        Err(ApiError::TooLarge)
    );
    assert_eq!(
        query_limit(&[("limit".to_owned(), "nope".to_owned())]),
        Err(ApiError::BadRequest)
    );
}

#[test]
fn query_value_requires_a_valid_identifier() {
    let query = vec![
        ("left".to_owned(), "snapshot-1".to_owned()),
        ("right".to_owned(), String::new()),
    ];
    assert_eq!(query_value(&query, "left"), Ok("snapshot-1"));
    assert_eq!(query_value(&query, "right"), Err(ApiError::BadRequest));
    assert_eq!(query_value(&query, "missing"), Err(ApiError::BadRequest));
    assert_eq!(
        query_value(&[("left".to_owned(), "-bad".to_owned())], "left"),
        Err(ApiError::BadRequest)
    );
}

#[test]
fn trim_cr_and_ascii_token_helpers() {
    assert_eq!(trim_cr(b"value\r"), b"value");
    assert_eq!(trim_cr(b"value"), b"value");
    assert_eq!(ascii_token(b"GET"), Ok("GET".to_owned()));
    assert_eq!(ascii_token(b""), Err(ApiError::BadRequest));
    assert_eq!(ascii_token(b"GE T"), Err(ApiError::BadRequest));
    assert_eq!(ascii_token(b"geT"), Ok("geT".to_owned()));
}

#[test]
fn read_request_bytes_waits_for_the_declared_body() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
    let address = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        let server = scope.spawn(|| {
            let (mut stream, _) = listener.accept().expect("accept");
            read_request_bytes(&mut stream)
        });
        let mut client = TcpStream::connect(address).expect("client");
        client
            .write_all(b"POST /ingest HTTP/1.1\r\nHost: 127.0.0.1:0\r\nContent-Length: 4\r\n\r\n")
            .expect("headers");
        client
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("probe timeout");
        let mut probe = [0_u8; 1];
        assert!(matches!(
            client.read(&mut probe),
            Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
        ));
        client.set_read_timeout(None).expect("clear timeout");
        client.write_all(b"body").expect("body");
        client.shutdown(Shutdown::Write).expect("close request");
        let bytes = server.join().expect("server thread").expect("bytes");
        assert_eq!(&bytes[bytes.len() - 4..], b"body");
    });
}
