// SPDX-License-Identifier: MIT

//! Restricted local ingestion and read projections for the Context Console.
//!
//! The service deliberately has no provider, game, process-execution, or gateway dependency.
//! `Store` accepts only immutable manifests and exposes scoped read operations. The
//! `integrated-demo` executable places a provider-free synthetic loopback transport around these
//! primitives for end-to-end review.

mod capture;
mod cli;
mod control;
mod integrated_demo;
mod observability;
mod private_store;
mod read_api;
mod store;

pub use capture::{
    CaptureConfig, CaptureError, CaptureMode, CaptureRecord, CaptureSink, MemoryCapture,
    NoopCapture, PreparedCapture, TransportState,
};
pub use cli::run_phase2_cli;
pub use control::{
    Boundary as ControlBoundary, CURRENT_DURABLE_STORE_SCHEMA_VERSION,
    Capabilities as ControlCapabilities, Command as ControlCommand, ControlError, ControlPlane,
    Draft as ControlDraft, DurableControlStore, DurableStoreError, DurableStoreFailpoint,
    DurableStoreSnapshot, EligibleItem as ControlEligibleItem, Event as ControlEvent,
    ItemRef as ControlItemRef, Operation as ControlOperation, Patch as ControlPatch,
    Preview as ControlPreview, PreviewComponent as ControlPreviewComponent,
    Receipt as ControlReceipt, Relation as ControlRelation, Revision as ControlRevision,
    Scope as ControlScope, State as ControlState,
};
pub use observability::{
    CaptureTelemetry, MemoryTelemetry, NoopTelemetry, TelemetryError, TelemetryExporter,
};
pub use private_store::{
    EncryptedContentMetadata, PolicyApproval, PrivateScope, PrivateStoreError, PrivateVault,
};
pub use read_api::{
    ApiError, HttpRequest, HttpResponse, MAX_HTTP_BODY_BYTES, MAX_HTTP_REQUEST_BYTES, ReadApi,
};
pub use store::{
    CapturePrivilege, CompareResult, ComponentSummary, EventPage, IngestError, MAX_COMPARE_BYTES,
    MAX_CONTENT_BYTES, MAX_CONTENT_REFS, MAX_EVENTS, MAX_MANIFEST_BYTES, MAX_SNAPSHOTS, ReadError,
    ReadGrant, SnapshotSummary, Store, StoreConfig,
};

pub use integrated_demo::run as run_integrated_demo;

/// Parse a management payload with the same bounded duplicate-key/depth checks used by the
/// integrated HTTP fixture. The CLI uses this helper so private note text arrives through stdin
/// and follows the exact control payload validation path.
pub fn parse_control_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ControlError> {
    integrated_demo::parse_json(body)
}

/// Run the deterministic, provider-free fixture demonstration.
pub fn demo() -> Result<(), IngestError> {
    let bytes = include_bytes!("../../../fixtures/valid/snapshot-cli.json");
    let metadata = include_bytes!("../../../fixtures/valid/snapshot-metadata.json");
    let mut store = Store::default();
    let summary = store.ingest(bytes)?;
    let metadata_summary = store.ingest(metadata)?;
    for (content_ref, content) in [
        (
            "blob-fixture-stdin",
            include_bytes!("../../../fixtures/blobs/fixture-stdin.txt").as_slice(),
        ),
        (
            "blob-fixture-schema",
            include_bytes!("../../../fixtures/blobs/fixture-output-schema.json").as_slice(),
        ),
        (
            "blob-fixture-config",
            include_bytes!("../../../fixtures/blobs/fixture-configuration.json").as_slice(),
        ),
        (
            "blob-fixture-http",
            include_bytes!("../../../fixtures/blobs/fixture-http-body.json").as_slice(),
        ),
    ] {
        store.ingest_content(content_ref, content)?;
    }
    let event_lines = include_bytes!("../../../fixtures/valid/events.jsonl");
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
