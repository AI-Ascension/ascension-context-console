// SPDX-License-Identifier: MIT

//! A provider-free producer → capture → read API → browser demonstration.
//!
//! `demo()` ingests the checked-in synthetic fixtures and prints the bounded read projection.
//! `run()` serves the same authenticated `ReadApi` over a loopback-only `/demo/*` surface so the
//! browser flow exercises the real store and API projection instead of reading fixture files
//! directly. No provider, game, URL fetch, or arbitrary process path is available here.

mod fixtures;
mod routes;
mod server;
mod state;

#[cfg(test)]
mod tests;

pub use server::run;

use crate::control::ControlError;
use crate::http::HttpRequest;
use crate::read_api::ReadApi;
use crate::store::{CapturePrivilege, IngestError, ReadGrant, Store};
use std::collections::BTreeSet;

const TOKEN: &[u8] = b"integrated-demo-token";
const PROJECT: &str = "agent-fixture-001";
const RUN: &str = "run-fixture-001";
const SNAPSHOT_ID: &str = "snapshot-fixture-metadata-001";
const COMPARISON_ID: &str = "snapshot-fixture-cli-001";
const EDITOR_TOKEN: &[u8] = b"fixture-editor-token";
const OBJECTIVE_TOKEN: &[u8] = b"fixture-objective-token";
const SESSION_TOKEN: &[u8] = b"fixture-session-token";
const SESSION_PRINCIPAL: &str = "fixture-session-reader";
const CSRF_TOKEN: &str = "fixture-csrf-token";
const DURABLE_STORE_KEY: [u8; 32] = [0x42; 32];

/// Run the deterministic, provider-free fixture demonstration.
pub fn demo() -> Result<(), IngestError> {
    let bytes = fixtures::CLI_SNAPSHOT;
    let metadata = fixtures::METADATA_SNAPSHOT;
    let mut store = Store::default();
    let summary = store.ingest(bytes)?;
    let metadata_summary = store.ingest(metadata)?;
    for (content_ref, content) in [
        ("blob-fixture-stdin", fixtures::BLOB_STDIN),
        ("blob-fixture-schema", fixtures::BLOB_OUTPUT_SCHEMA),
        ("blob-fixture-config", fixtures::BLOB_CONFIGURATION),
        ("blob-fixture-http", fixtures::BLOB_HTTP_BODY),
    ] {
        store.ingest_content(content_ref, content)?;
    }
    let event_lines = fixtures::EVENTS;
    let mut event_count = 0_usize;
    for line in event_lines
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        store.append_event(line)?;
        event_count += 1;
    }
    let grant = ReadGrant::issue(
        b"offline-demo-token",
        "agent-fixture-001",
        Some("run-fixture-001".to_owned()),
        CapturePrivilege::Content,
        std::time::Duration::from_secs(600),
        std::time::UNIX_EPOCH,
    )
    .map_err(|_| IngestError::Capacity)?;
    let api = ReadApi::new(
        &store,
        &grant,
        b"offline-demo-token",
        "127.0.0.1:0",
        Some("http://127.0.0.1:0".to_owned()),
        std::time::UNIX_EPOCH,
    );
    let response = api.handle_at(
        &HttpRequest {
            method: "GET".to_owned(),
            target: "/v1/runs/run-fixture-001/snapshots".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:0".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:0".to_owned()),
                (
                    "authorization".to_owned(),
                    "Bearer offline-demo-token".to_owned(),
                ),
            ],
            body: Vec::new(),
        },
        std::time::UNIX_EPOCH,
    );
    println!("offline_demo=true");
    println!("provider_calls=0");
    println!("game_launches=0");
    println!("snapshot_id={}", summary.snapshot_id);
    println!("metadata_snapshot_id={}", metadata_summary.snapshot_id);
    println!("boundary={}", summary.boundary);
    println!("capture_mode={}", summary.capture_mode);
    println!("component_count={}", summary.component_count);
    println!(
        "application_capture_complete={}",
        summary.application_capture_complete
    );
    println!("retained_events={event_count}");
    println!("read_api_status={}", response.status);
    println!("read_api_is_mutation_free=true");
    Ok(())
}

fn durable_error(_: crate::control::DurableStoreError) -> ControlError {
    ControlError::invalid(
        "durable_store_unavailable",
        "control state could not be durably persisted",
    )
}

pub(crate) fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ControlError> {
    if body.is_empty() || body.len() > 16 * 1024 {
        return Err(ControlError::invalid(
            "body_too_large",
            "management body exceeds its bound",
        ));
    }
    validate_json_shape(body)?;
    serde_json::from_slice(body)
        .map_err(|_| ControlError::invalid("invalid_json", "management JSON is invalid"))
}

fn validate_json_shape(body: &[u8]) -> Result<(), ControlError> {
    let mut parser = JsonGuard {
        bytes: body,
        offset: 0,
    };
    parser.value(0)?;
    parser.space();
    if parser.offset != body.len() {
        return Err(ControlError::invalid(
            "invalid_json",
            "management JSON has trailing data",
        ));
    }
    Ok(())
}

struct JsonGuard<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl JsonGuard<'_> {
    fn value(&mut self, depth: usize) -> Result<(), ControlError> {
        if depth > 32 {
            return Err(ControlError::invalid(
                "json_depth",
                "management JSON is too deeply nested",
            ));
        }
        self.space();
        match self.bytes.get(self.offset).copied() {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(_) => self.primitive(),
            None => Err(ControlError::invalid(
                "invalid_json",
                "management JSON is incomplete",
            )),
        }
    }

    fn object(&mut self, depth: usize) -> Result<(), ControlError> {
        self.offset += 1;
        self.space();
        let mut keys = BTreeSet::new();
        if self.take(b'}') {
            return Ok(());
        }
        loop {
            self.space();
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(ControlError::invalid(
                    "duplicate_json_key",
                    "duplicate JSON keys are rejected",
                ));
            }
            self.space();
            if !self.take(b':') {
                return Err(ControlError::invalid(
                    "invalid_json",
                    "object member separator is missing",
                ));
            }
            self.value(depth)?;
            self.space();
            if self.take(b'}') {
                return Ok(());
            }
            if !self.take(b',') {
                return Err(ControlError::invalid(
                    "invalid_json",
                    "object delimiter is missing",
                ));
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<(), ControlError> {
        self.offset += 1;
        self.space();
        if self.take(b']') {
            return Ok(());
        }
        let mut count = 0_usize;
        loop {
            count = count.saturating_add(1);
            if count > 256 {
                return Err(ControlError::invalid(
                    "json_items",
                    "management JSON array is too large",
                ));
            }
            self.value(depth)?;
            self.space();
            if self.take(b']') {
                return Ok(());
            }
            if !self.take(b',') {
                return Err(ControlError::invalid(
                    "invalid_json",
                    "array delimiter is missing",
                ));
            }
        }
    }

    fn primitive(&mut self) -> Result<(), ControlError> {
        let start = self.offset;
        while let Some(byte) = self.bytes.get(self.offset).copied() {
            if byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}') {
                break;
            }
            self.offset += 1;
        }
        (self.offset > start)
            .then_some(())
            .ok_or_else(|| ControlError::invalid("invalid_json", "JSON value is incomplete"))
    }

    fn string(&mut self) -> Result<String, ControlError> {
        let start = self.offset;
        if !self.take(b'"') {
            return Err(ControlError::invalid(
                "invalid_json",
                "JSON string is missing",
            ));
        }
        let mut escaped = false;
        while let Some(byte) = self.bytes.get(self.offset).copied() {
            self.offset += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                return serde_json::from_slice(&self.bytes[start..self.offset])
                    .map_err(|_| ControlError::invalid("invalid_json", "JSON string is invalid"));
            }
        }
        Err(ControlError::invalid(
            "invalid_json",
            "JSON string is unterminated",
        ))
    }

    fn space(&mut self) {
        while self
            .bytes
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
        }
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.bytes.get(self.offset).copied() == Some(expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
}
