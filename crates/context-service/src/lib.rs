// SPDX-License-Identifier: MIT

//! Restricted local ingestion and read projections for the Context Console.
//!
//! The service deliberately has no provider, game, process-execution, or gateway dependency.
//! `Store` accepts only immutable manifests and exposes scoped read operations. The
//! `integrated-demo` executable places a provider-free synthetic loopback transport around these
//! primitives for end-to-end review.

mod capture;
mod demo;
mod http;
mod private_store;
mod read_api;
mod store;
mod telemetry;

pub use capture::{
    CaptureConfig, CaptureError, CaptureMode, CaptureRecord, CaptureSink, MemoryCapture,
    NoopCapture, PreparedCapture, TransportState,
};
pub use demo::{demo, run as run_integrated_demo};
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
pub use telemetry::{
    CaptureTelemetry, MemoryTelemetry, NoopTelemetry, TelemetryError, TelemetryExporter,
};
