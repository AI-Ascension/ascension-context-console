// SPDX-License-Identifier: MIT

//! Restricted local ingestion and read projections for the Context Console.
//!
//! The service deliberately has no provider, game, process-execution, or gateway dependency.
//! `Store` accepts only immutable manifests and exposes scoped read operations. The
//! `integrated-demo` executable places a provider-free synthetic loopback transport around these
//! primitives for end-to-end review.

mod association;
mod capture;
mod cli;
mod control;
mod demo;
pub mod effective_limits;
mod harness_facade;
mod http;
mod memory;
mod owner;
mod private_store;
pub mod provider_session;
mod read_api;
mod store;
mod telemetry;

pub use association::{
    AssociationResolutionError, ResolvedWorkflowContext, resolve_workflow_context_association,
};
pub use capture::{
    CaptureConfig, CaptureError, CaptureMode, CaptureRecord, CaptureSink, MemoryCapture,
    NoopCapture, PreparedCapture, TransportState,
};
pub use cli::{run_phase2_cli, run_phase3_adapter, run_phase3_cli, run_phase4_cli};
pub use control::{
    Boundary as ControlBoundary, CURRENT_DURABLE_STORE_SCHEMA_VERSION,
    Capabilities as ControlCapabilities, Command as ControlCommand, ControlError, ControlPlane,
    Draft as ControlDraft, DurableControlStore, DurableStoreError, DurableStoreFailpoint,
    DurableStoreSnapshot, EligibleItem as ControlEligibleItem, Event as ControlEvent,
    ItemRef as ControlItemRef, MemoryBindingRecord as ControlMemoryBindingRecord,
    Operation as ControlOperation, Patch as ControlPatch, Preview as ControlPreview,
    PreviewComponent as ControlPreviewComponent, Receipt as ControlReceipt,
    Relation as ControlRelation, Revision as ControlRevision, Scope as ControlScope,
    State as ControlState,
};
pub use demo::{demo, run as run_integrated_demo};
pub use harness_facade::{
    CapabilityGrant, FACADE_CAPABILITIES_SCHEMA, FACADE_CONFIG_SCHEMA, FACADE_ERROR_SCHEMA,
    FacadeCacheStatus, FacadeCapabilities, FacadeCaptureMode, FacadeConfigError, FacadeError,
    FacadeErrorClass, FacadePermission, FacadeRequest, FacadeResult, GrantError, GrantRegistry,
    HarnessBackedContextService, HarnessFacadeConfig, HarnessOwnerClient, HarnessOwnerPort,
    MAX_FACADE_BODY_BYTES, MAX_FACADE_CACHE_ENTRIES, MAX_FACADE_HOST_BYTES, MAX_FACADE_ID_BYTES,
    MAX_FACADE_ITEM_CACHE_ENTRIES, MAX_FACADE_ORIGIN_BYTES, MAX_FACADE_PRINCIPAL_BYTES,
    MAX_FACADE_RESPONSE_BYTES, MAX_FACADE_TOKEN_BYTES, OwnerAuthorization, OwnerError,
    PreviewRequest, ProtectedAuthReference, RetentionMode, RetentionPolicy, SecretDigest,
};
pub use memory::{
    AdvertisedMemoryCapabilities, CapabilitiesReadError, MAX_MEMORY_BODY_BYTES,
    MAX_MEMORY_QUERY_BYTES, MEMORY_CAPABILITIES_SCHEMA, MEMORY_CAPABILITIES_SCHEMA_V1,
    MemoryBinding, MemoryCapabilities, MemoryCapabilitiesV1, MemoryEffectiveLimits,
    MemoryQueryRequest, MemoryRoute, MemoryRouteError, MemoryScope,
    read_advertised_memory_capabilities,
};
pub use owner::{
    HarnessOwner, HarnessOwnerComposition, OWNER_MAX_BODY_BYTES, OWNER_MAX_RESPONSE_BYTES,
    OWNER_MAX_RESPONSE_DEPTH, OWNER_RECEIPT_SCHEMA, OwnerAuthError, OwnerCall, OwnerConfigError,
    OwnerError as OwnerPortError, OwnerGrant, OwnerGrantBook, OwnerGrantClass, OwnerGrantReceipt,
    OwnerOperation, OwnerOutcome, OwnerReceipt, OwnerReceiptLookup, OwnerReply,
    OwnerRequestContext, OwnerResponseError, OwnerScope,
};
pub use private_store::{
    EncryptedContentMetadata, PolicyApproval, PrivateScope, PrivateStoreError, PrivateVault,
};
pub use provider_session::{
    AdvertisedSessionCapabilities, ProviderSessionRoute, SESSION_CAPABILITIES_SCHEMA,
    SESSION_CAPABILITIES_SCHEMA_V1, SessionApiError, SessionBinding, SessionBindingView,
    SessionCapabilitiesReadError, SessionCapabilitiesV1, SessionCapabilitiesView,
    SessionEffectiveLimits, SessionHardeningView, SessionOperationView, SessionRouteMode,
    SessionScopeView, read_advertised_session_capabilities,
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

/// Parse a management payload with the same bounded duplicate-key/depth checks used by the
/// integrated HTTP fixture. The CLI uses this helper so private note text arrives through stdin
/// and follows the exact control payload validation path.
pub fn parse_control_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ControlError> {
    demo::parse_json(body)
}
