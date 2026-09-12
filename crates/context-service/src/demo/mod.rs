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

use crate::http::HttpRequest;
use crate::read_api::ReadApi;
use crate::store::{CapturePrivilege, IngestError, ReadGrant, Store};

const TOKEN: &[u8] = b"integrated-demo-token";
const PROJECT: &str = "agent-fixture-001";
const RUN: &str = "run-fixture-001";
const SNAPSHOT_ID: &str = "snapshot-fixture-metadata-001";
const COMPARISON_ID: &str = "snapshot-fixture-cli-001";

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
