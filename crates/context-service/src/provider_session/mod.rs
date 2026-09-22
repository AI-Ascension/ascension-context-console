// SPDX-License-Identifier: MIT

//! Authenticated, bounded client projection for Phase 4 provider sessions.
//!
//! The target repository is a console, not the session authority.  This fixture route mirrors the
//! typed product namespace and records only operation metadata; native IDs, credentials, provider
//! RPC methods and turn submission are never accepted from a caller.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::effective_limits::{
    EFFECTIVE_LIMIT_RECORD_SCHEMA, EffectiveLimitRecord, LimitRow, UnavailableReason, contract_pins,
};
use crate::owner::{
    HarnessOwnerComposition, OwnerError, OwnerGrantBook, OwnerOperation, OwnerRequestContext,
    owner_call, owner_scope, validate_public_value,
};

mod capabilities;
#[cfg(test)]
mod capabilities_tests;
mod route;
mod service;
mod support;

pub use capabilities::read_advertised_session_capabilities;

pub const SESSION_API_SCHEMA: &str = "ascension.provider-session.api-result.v1";
/// Advertised capability schema. The `v3` contract additionally requires the `effective_limits`
/// and `binding` objects. The `v1` payload shape remains readable through the dual reader.
pub const SESSION_CAPABILITIES_SCHEMA: &str = "ascension.provider-session.capabilities.v3";
/// Legacy capability schema preserved for dual reading during migration.
pub const SESSION_CAPABILITIES_SCHEMA_V1: &str = "ascension.provider-session.capabilities.v1";
const MAX_BODY_BYTES: usize = 16 * 1024;
const MAX_BINDINGS: usize = 128;
const MAX_OPERATIONS: usize = 512;
const MAX_CANDIDATES: usize = 4;
const FIXTURE_EXPIRY_SECONDS: u64 = 900;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionApiError {
    BadRequest,
    Unauthorized,
    AuthNeeded,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    Unsupported,
    Capacity,
    Conflict,
    Stale,
    Expired,
    Ambiguous,
    Unavailable,
    MalformedPeer,
    EffectiveLimit(UnavailableReason),
}

impl std::fmt::Display for SessionApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BadRequest => "provider-session request is invalid",
            Self::Unauthorized => "provider-session authorization is required",
            Self::AuthNeeded => "provider-session authentication is needed",
            Self::Forbidden => "provider-session operation is forbidden",
            Self::NotFound => "provider-session resource is unavailable",
            Self::MethodNotAllowed => "provider-session method is not allowed",
            Self::Unsupported => "provider-session capability is unsupported",
            Self::Capacity => "provider-session capacity exceeded",
            Self::Conflict => "provider-session request conflicts",
            Self::Stale => "provider-session view is stale",
            Self::Expired => "provider-session authorization or preview expired",
            Self::Ambiguous => "provider-session operation outcome is ambiguous",
            Self::Unavailable => "provider-session provider state is unavailable",
            Self::MalformedPeer => "provider-session peer response is malformed",
            Self::EffectiveLimit(reason) => reason.code(),
        })
    }
}

impl std::error::Error for SessionApiError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRouteMode {
    Disabled,
    FixtureOnly,
    InspectOnly,
    Enabled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionHardeningView {
    pub tools_enabled: bool,
    pub ambient_history: bool,
    pub encrypted_state: bool,
    pub configuration_verified: bool,
    pub transform_handling: String,
}

/// Canonical `effective_limits` object required by the `provider-session` capability `v3` schema.
///
/// The fixture values are synthetic and equal to the portable schema ceilings; they are not a
/// claim about a live provider, native peer, or harness owner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEffectiveLimits {
    pub policy_schema: String,
    pub max_session_items: u64,
    pub max_dependencies: u64,
    pub max_events: u64,
    pub max_operations: u64,
    pub max_prepared: u64,
    pub max_candidates: u64,
    pub max_maintenance_jobs: u64,
    pub max_completed_turns: u64,
    pub max_history_ttl_seconds: u64,
    pub max_frame_bytes: u64,
    pub max_history_bytes: u64,
    pub max_prepared_bytes: u64,
    pub max_suffix_bytes: u64,
    pub max_output_schema_bytes: u64,
    pub max_method_bytes: u64,
    pub max_json_depth: u64,
}

/// Owner/adapter identity required by the `provider-session` capability `v3` schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub owner: String,
    pub owner_revision: String,
    pub policy_schema_sha256: String,
    pub model_revision: String,
    pub adapter_revision: String,
    pub adapter_revision_sha256: String,
    pub descriptor_sha256: String,
}

/// Advertised `v3` provider-session capability descriptor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCapabilitiesView {
    pub schema: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub native_version: String,
    pub native_binary_sha256: String,
    pub native_schema_sha256: String,
    pub evidence: String,
    pub transport: String,
    pub enabled_methods: Vec<String>,
    pub hardening: SessionHardeningView,
    pub effective_limits: SessionEffectiveLimits,
    pub binding: SessionBinding,
    pub strict_executable: bool,
    pub experimental_api: bool,
    pub unknown_methods: String,
    pub raw_rpc: bool,
}

/// Legacy `v1` capability descriptor, still readable through the dual reader.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCapabilitiesV1 {
    pub schema: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub native_version: String,
    pub native_binary_sha256: String,
    pub native_schema_sha256: String,
    pub evidence: String,
    pub transport: String,
    pub enabled_methods: Vec<String>,
    pub hardening: SessionHardeningView,
    pub strict_executable: bool,
    pub experimental_api: bool,
    pub unknown_methods: String,
    pub raw_rpc: bool,
}

/// Result of the provider-session dual reader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdvertisedSessionCapabilities {
    V1(Box<SessionCapabilitiesV1>),
    V3(Box<SessionCapabilitiesView>),
}

impl AdvertisedSessionCapabilities {
    #[must_use]
    pub fn schema(&self) -> &str {
        match self {
            Self::V1(capabilities) => &capabilities.schema,
            Self::V3(capabilities) => &capabilities.schema,
        }
    }

    #[must_use]
    pub fn effective_limits(&self) -> Option<&SessionEffectiveLimits> {
        match self {
            Self::V1(_) => None,
            Self::V3(capabilities) => Some(&capabilities.effective_limits),
        }
    }
}

/// Why a provider-session capability descriptor could not be read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionCapabilitiesReadError {
    UnknownSchema,
    Malformed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionScopeView {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBindingView {
    pub schema: String,
    pub binding_id: String,
    pub scope: SessionScopeView,
    pub branch_id: String,
    pub credential_realm_ref: String,
    pub profile_sha256: String,
    pub owner_epoch: u64,
    pub session_epoch: u64,
    // Public and non-sensitive: `run_id`/`dependency_count` are omitted from the wire form only
    // because `scope.run_id` and `dependency_ids` already carry them. Unlike `ControlPlane`, these
    // fields are readable by any caller, so the derived `Debug` discloses nothing new and is kept.
    #[serde(skip)]
    pub run_id: String,
    pub state: String,
    pub purpose: String,
    pub native_thread_ref: String,
    pub dependency_ids: Vec<String>,
    pub continuity_sha256: String,
    pub history_coverage: String,
    pub history_epoch: u64,
    pub compaction_epoch: u64,
    #[serde(skip)]
    pub dependency_count: usize,
    pub game_dispatch_capability: bool,
    pub expires_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOperationView {
    pub schema: String,
    pub operation_id: String,
    pub scope: SessionScopeView,
    pub binding_id: String,
    pub kind: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub state: String,
    pub owner_epoch: u64,
    pub session_epoch: u64,
    pub generation_permission: bool,
    pub generation_class: bool,
    pub automatic_retry: bool,
    pub auto_resume: bool,
    pub game_effects: u64,
    pub terminal_evidence_ref: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProviderSessionRoute {
    principal: String,
    mode: SessionRouteMode,
    capabilities: SessionCapabilitiesView,
    bindings: BTreeMap<String, SessionBindingView>,
    operations: BTreeMap<String, SessionOperationView>,
    idempotency: BTreeMap<String, IdempotencyRecord>,
    next_id: u64,
    owner: Option<HarnessOwnerComposition>,
    attached_scope: Option<SessionScopeView>,
    attached_bindings: BTreeMap<String, SessionScopeView>,
    attached_operations: BTreeMap<String, SessionScopeView>,
    capability_version: crate::CapabilityVersion,
}

#[derive(Clone, Debug)]
struct IdempotencyRecord {
    request_sha256: String,
    operation_id: String,
    binding_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionCandidatePurpose {
    ExecutableCandidate,
    Evaluation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCandidateCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    approved_policy_ref: String,
    profile_ref: String,
    purpose: SessionCandidatePurpose,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionHistoryRefreshCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    expected_session_epoch: u64,
    expected_history_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionReconnectCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    expected_session_epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionForkOperation {
    NativeFork,
    CleanRehydration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionForkPlanCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    expected_session_epoch: u64,
    expected_history_epoch: u64,
    cutoff_turn_ref: String,
    operation: SessionForkOperation,
    purpose: SessionForkPurpose,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionForkPurpose {
    Evaluation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionCompactionPolicy {
    StrictReviewed,
    ObservedPersistent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCompactionPlanCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    expected_session_epoch: u64,
    expected_history_epoch: u64,
    requested_policy: SessionCompactionPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionPreparedBindingCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    expected_session_epoch: u64,
    expected_history_epoch: u64,
    phase2_draft_ref: String,
    phase3_selection_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionForkCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    approved_fork_plan_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCompactionCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    approved_compaction_plan_ref: String,
    spend_authorization_ref: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionRetireReason {
    OperatorRequest,
    SourceRemoved,
    ContinuityLost,
    ScopeEnded,
    CapacityRotation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRetireCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    expected_session_epoch: u64,
    reason: SessionRetireReason,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionCleanupCommand {
    idempotency_key: String,
    expected_control_generation: u64,
    retirement_ref: String,
    erase_authorization_ref: String,
    expected_session_epoch: u64,
}
