// SPDX-License-Identifier: MIT

use super::*;
use crate::store::{CapturePrivilege, ReadGrant, Store};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::time::{Duration, UNIX_EPOCH};

const FIXTURE: &[u8] = include_bytes!("../../../../fixtures/synthetic/snapshot.json");

fn api() -> (Store, ReadGrant) {
    let mut store = Store::default();
    store.ingest(FIXTURE).expect("fixture");
    let grant = ReadGrant::issue(
        b"api-token",
        "agent-t02-reader",
        None,
        CapturePrivilege::Metadata,
        Duration::from_secs(60),
        UNIX_EPOCH,
    )
    .expect("grant");
    (store, grant)
}

#[test]
fn health_is_minimal_and_mutation_is_rejected() {
    let (store, grant) = api();
    let api = ReadApi::new(
        &store,
        &grant,
        b"api-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        UNIX_EPOCH,
    );
    let health = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/health".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
            ],
            body: Vec::new(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(health.status, 200);
    let post = api.handle_at(
        &HttpRequest {
            method: "POST".to_owned(),
            target: "/v1/capabilities".to_owned(),
            headers: vec![("host".to_owned(), "127.0.0.1:0".to_owned())],
            body: Vec::new(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(post.status, 405);
}

#[test]
fn url_tokens_and_wrong_origins_are_rejected() {
    let (store, grant) = api();
    let api = ReadApi::new(
        &store,
        &grant,
        b"api-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        UNIX_EPOCH,
    );
    let response = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/v1/capabilities?token=api-token".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                ("authorization".to_owned(), "Bearer api-token".to_owned()),
            ],
            body: Vec::new(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(response.status, 400);
    let response = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/v1/capabilities?%74oken=api-token".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                ("authorization".to_owned(), "Bearer api-token".to_owned()),
            ],
            body: Vec::new(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(response.status, 400);
    let response = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/v1/capabilities".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "https://evil.invalid".to_owned()),
                ("authorization".to_owned(), "Bearer api-token".to_owned()),
            ],
            body: Vec::new(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(response.status, 403);
}

#[test]
fn reused_api_checks_capability_expiry_at_each_request() {
    let (store, grant) = api();
    let api = ReadApi::new(
        &store,
        &grant,
        b"api-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        UNIX_EPOCH,
    );
    let request = |target: &str| HttpRequest {
        method: "GET".to_owned(),
        target: target.to_owned(),
        headers: vec![
            ("host".to_owned(), "127.0.0.1:0".to_owned()),
            ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
            ("authorization".to_owned(), "Bearer api-token".to_owned()),
        ],
        body: Vec::new(),
    };
    assert_eq!(
        api.handle_at(&request("/v1/capabilities"), UNIX_EPOCH)
            .status,
        200
    );
    assert_eq!(
        api.handle_at(
            &request("/v1/capabilities"),
            UNIX_EPOCH + Duration::from_secs(60),
        )
        .status,
        401
    );
}

#[test]
fn capabilities_honor_revocation() {
    let (mut store, grant) = api();
    store.revoke(b"api-token").expect("revoke");
    let api = ReadApi::new(
        &store,
        &grant,
        b"api-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        UNIX_EPOCH,
    );
    let response = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/v1/capabilities".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                ("authorization".to_owned(), "Bearer api-token".to_owned()),
            ],
            body: Vec::new(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(response.status, 403);
}

#[test]
fn get_body_is_rejected_even_when_within_the_bound() {
    let (store, grant) = api();
    let api = ReadApi::new(
        &store,
        &grant,
        b"api-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        UNIX_EPOCH,
    );
    let response = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/health".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
            ],
            body: b"unexpected body".to_vec(),
        },
        UNIX_EPOCH,
    );
    assert_eq!(response.status, 400);
}

#[test]
fn serve_once_rejects_a_declared_body_sent_after_the_headers() {
    let (store, grant) = api();
    let api = ReadApi::new(
        &store,
        &grant,
        b"api-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        UNIX_EPOCH,
    );
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
    let address = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        let server = scope.spawn(|| api.serve_once(&listener));
        let mut client = TcpStream::connect(address).expect("client");
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1:0\r\nContent-Length: 4\r\n\r\n")
            .expect("request");
        client
            .set_read_timeout(Some(Duration::from_millis(100)))
            .expect("probe timeout");
        let mut probe = [0_u8; 1];
        assert!(
            matches!(client.read(&mut probe), Err(error) if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock))
        );
        client.set_read_timeout(None).expect("clear timeout");
        client.write_all(b"body").expect("body");
        client.shutdown(Shutdown::Write).expect("close request");
        let mut response = Vec::new();
        client.read_to_end(&mut response).expect("response");
        assert!(response.starts_with(b"HTTP/1.1 400 Bad Request\r\n"));
        assert!(server.join().expect("server thread").is_ok());
    });
}
