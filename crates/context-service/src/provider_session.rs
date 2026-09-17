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

/// Dual reader: a valid `v1` payload still reads, and a `v3` payload reads with effective limits.
///
/// # Errors
///
/// Returns [`SessionCapabilitiesReadError::UnknownSchema`] for an unrecognized schema and
/// [`SessionCapabilitiesReadError::Malformed`] for a payload that does not match the named version.
pub fn read_advertised_session_capabilities(
    bytes: &[u8],
) -> Result<AdvertisedSessionCapabilities, SessionCapabilitiesReadError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| SessionCapabilitiesReadError::Malformed)?;
    match value.get("schema").and_then(Value::as_str) {
        Some(SESSION_CAPABILITIES_SCHEMA_V1) => {
            serde_json::from_value::<SessionCapabilitiesV1>(value)
                .map(|capabilities| AdvertisedSessionCapabilities::V1(Box::new(capabilities)))
                .map_err(|_| SessionCapabilitiesReadError::Malformed)
        }
        Some(SESSION_CAPABILITIES_SCHEMA) => {
            serde_json::from_value::<SessionCapabilitiesView>(value)
                .map(|capabilities| AdvertisedSessionCapabilities::V3(Box::new(capabilities)))
                .map_err(|_| SessionCapabilitiesReadError::Malformed)
        }
        _ => Err(SessionCapabilitiesReadError::UnknownSchema),
    }
}

/// Portable `policy.v1` ceilings for the two policy-backed provider-session values.
const SESSION_POLICY_SCHEMA_MAX_COMPLETED_TURNS: u64 = 1024;
const SESSION_POLICY_SCHEMA_MAX_HISTORY_TTL_SECONDS: u64 = 604_800;

impl SessionCapabilitiesView {
    /// The legacy `v1` advertisement derived from this `v3` target descriptor. The served route
    /// keeps using this shape until the coordinated `v3` cutover.
    #[must_use]
    pub fn to_v1(&self) -> SessionCapabilitiesV1 {
        SessionCapabilitiesV1 {
            schema: SESSION_CAPABILITIES_SCHEMA_V1.to_owned(),
            profile_id: self.profile_id.clone(),
            profile_sha256: self.profile_sha256.clone(),
            native_version: self.native_version.clone(),
            native_binary_sha256: self.native_binary_sha256.clone(),
            native_schema_sha256: self.native_schema_sha256.clone(),
            evidence: self.evidence.clone(),
            transport: self.transport.clone(),
            enabled_methods: self.enabled_methods.clone(),
            hardening: self.hardening.clone(),
            strict_executable: self.strict_executable,
            experimental_api: self.experimental_api,
            unknown_methods: self.unknown_methods.clone(),
            raw_rpc: self.raw_rpc,
        }
    }

    /// Derivation of the trusted effective-limit record from this validated capability
    /// descriptor. The record-under-test must be authenticated against this derivation, never
    /// the other way around.
    ///
    /// Schema ceilings below are fixed by the pinned policy/capability artifacts, independently
    /// of the selected limits. Producer-generated conformance vectors check all rows, including
    /// restricted and disabled profiles; the caller still supplies a trusted descriptor.
    #[must_use]
    pub fn effective_limit_record(&self) -> EffectiveLimitRecord {
        let limits = &self.effective_limits;
        EffectiveLimitRecord {
            schema: EFFECTIVE_LIMIT_RECORD_SCHEMA.to_owned(),
            surface: "provider-session".to_owned(),
            owner: self.binding.owner.clone(),
            owner_revision: self.binding.owner_revision.clone(),
            capability_schema: self.schema.clone(),
            capability_descriptor_sha256: self.binding.descriptor_sha256.clone(),
            enabled: !self.enabled_methods.is_empty(),
            rows: vec![
                LimitRow::policy(
                    "max_completed_turns",
                    SESSION_POLICY_SCHEMA_MAX_COMPLETED_TURNS,
                    128,
                    limits.max_completed_turns,
                    "ProviderSessionPolicy::validate_schema+ProviderSessionBroker::new",
                ),
                LimitRow::policy(
                    "max_history_ttl_seconds",
                    SESSION_POLICY_SCHEMA_MAX_HISTORY_TTL_SECONDS,
                    86_400,
                    limits.max_history_ttl_seconds,
                    "ProviderSessionPolicy::validate_schema+ProviderSessionBroker::new",
                ),
                LimitRow::runtime_guard(
                    "max_session_items",
                    512,
                    limits.max_session_items,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_dependencies",
                    128,
                    limits.max_dependencies,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_events",
                    4096,
                    limits.max_events,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_operations",
                    1024,
                    limits.max_operations,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_prepared",
                    1024,
                    limits.max_prepared,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_candidates",
                    4,
                    limits.max_candidates,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_maintenance_jobs",
                    2,
                    limits.max_maintenance_jobs,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_frame_bytes",
                    262_144,
                    limits.max_frame_bytes,
                    "NativeTransport",
                ),
                LimitRow::runtime_guard(
                    "max_history_bytes",
                    4_194_304,
                    limits.max_history_bytes,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_prepared_bytes",
                    4_194_304,
                    limits.max_prepared_bytes,
                    "ProviderSessionBroker",
                ),
                LimitRow::runtime_guard(
                    "max_suffix_bytes",
                    131_072,
                    limits.max_suffix_bytes,
                    "NativeTransport",
                ),
                LimitRow::runtime_guard(
                    "max_output_schema_bytes",
                    65_536,
                    limits.max_output_schema_bytes,
                    "NativeTransport",
                ),
                LimitRow::runtime_guard(
                    "max_method_bytes",
                    128,
                    limits.max_method_bytes,
                    "NativeFrame parse",
                ),
                LimitRow::runtime_guard(
                    "max_json_depth",
                    64,
                    limits.max_json_depth,
                    "NativeFrame parse",
                ),
            ],
        }
    }

    /// Admit a session policy value against this descriptor's executable ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when disabled, absent, or above the executable ceiling.
    pub fn admit_policy_value(&self, field: &str, requested: u64) -> Result<(), UnavailableReason> {
        self.effective_limit_record().admit(field, requested)
    }

    /// Admit a value only after authenticating the published record against this trusted
    /// descriptor's derivation.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when the record is stale, tampered, targets another profile,
    /// or the value exceeds the executable ceiling.
    pub fn admit_authorized_record(
        &self,
        record: &EffectiveLimitRecord,
        field: &str,
        requested: u64,
    ) -> Result<(), UnavailableReason> {
        record.admit_authorized(&self.effective_limit_record(), field, requested)
    }
}

fn fixture_session_effective_limits() -> SessionEffectiveLimits {
    SessionEffectiveLimits {
        policy_schema: "ascension.provider-session.policy.v1".to_owned(),
        max_session_items: 512,
        max_dependencies: 128,
        max_events: 4096,
        max_operations: 1024,
        max_prepared: 1024,
        max_candidates: 4,
        max_maintenance_jobs: 2,
        max_completed_turns: 128,
        max_history_ttl_seconds: 86_400,
        max_frame_bytes: 262_144,
        max_history_bytes: 4_194_304,
        max_prepared_bytes: 4_194_304,
        max_suffix_bytes: 131_072,
        max_output_schema_bytes: 65_536,
        max_method_bytes: 128,
        max_json_depth: 64,
    }
}

fn fixture_session_binding() -> SessionBinding {
    SessionBinding {
        owner: "sts2-harness".to_owned(),
        owner_revision: "harness-provider-session-v3".to_owned(),
        policy_schema_sha256: contract_pins::SESSION_POLICY_SCHEMA_SHA256.to_owned(),
        model_revision: "fixture-peer-1".to_owned(),
        adapter_revision: "codex-app-server-fixture-v1".to_owned(),
        adapter_revision_sha256: sha256_hex("codex-app-server-fixture-v1"),
        descriptor_sha256: String::new(),
    }
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

impl ProviderSessionRoute {
    #[must_use]
    pub fn fixture(principal: impl Into<String>) -> Self {
        let mut route = Self {
            principal: principal.into(),
            mode: SessionRouteMode::FixtureOnly,
            capabilities: SessionCapabilitiesView {
                schema: SESSION_CAPABILITIES_SCHEMA.to_owned(),
                profile_id: "codex-app-server-fixture-v1".to_owned(),
                profile_sha256: sha256_hex("codex-app-server-fixture-v1"),
                native_version: "fixture-peer-1".to_owned(),
                native_binary_sha256: sha256_hex("compiled-fake-native-peer"),
                native_schema_sha256: sha256_hex("codex-app-server-jsonrpc.v2"),
                evidence: "compiled_peer".to_owned(),
                transport: "owned_stdio".to_owned(),
                enabled_methods: vec![
                    "initialize".to_owned(),
                    "thread/start".to_owned(),
                    "thread/read".to_owned(),
                    "turn/start".to_owned(),
                    "turn/interrupt".to_owned(),
                    "thread/fork".to_owned(),
                    "thread/compact/start".to_owned(),
                ],
                hardening: SessionHardeningView {
                    tools_enabled: false,
                    ambient_history: false,
                    // Widened from `const true` to `boolean` by the producer. The console keeps its
                    // independent private-retention guard (accepted policy approval plus
                    // authenticated encryption) regardless of this advertised value.
                    encrypted_state: false,
                    configuration_verified: true,
                    transform_handling: "detect_and_fence".to_owned(),
                },
                effective_limits: fixture_session_effective_limits(),
                binding: fixture_session_binding(),
                strict_executable: false,
                experimental_api: false,
                unknown_methods: "deny".to_owned(),
                raw_rpc: false,
            },
            bindings: BTreeMap::new(),
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            next_id: 1,
            owner: None,
            attached_scope: None,
            attached_bindings: BTreeMap::new(),
            attached_operations: BTreeMap::new(),
            capability_version: crate::CapabilityVersion::V3,
        };
        // Serialization failure leaves an invalid empty digest; the served route fails closed.
        if let Ok(digest) = route.capabilities.descriptor_digest() {
            route.capabilities.binding.descriptor_sha256 = digest;
        }
        route
    }

    /// Builds the explicitly attached composition. The route owns no native session state in this
    /// mode; all session, history and compaction decisions are delegated to `composition`.
    #[must_use]
    pub fn attached(principal: impl Into<String>, composition: HarnessOwnerComposition) -> Self {
        let mut route = Self::fixture(principal);
        route.mode = SessionRouteMode::Enabled;
        route.owner = Some(composition);
        route.bindings.clear();
        route.operations.clear();
        route.idempotency.clear();
        route
    }

    /// Attached constructor with an explicit owner scope. This is the production composition
    /// entry point; the compatibility constructor above uses the historical fixture scope.
    #[must_use]
    pub fn attached_with_scope(
        principal: impl Into<String>,
        scope: SessionScopeView,
        composition: HarnessOwnerComposition,
    ) -> Self {
        let mut route = Self::attached(principal, composition);
        route.attached_scope = Some(scope);
        route
    }

    #[must_use]
    pub fn with_owner(
        principal: impl Into<String>,
        owner: std::sync::Arc<dyn crate::owner::HarnessOwner>,
        grants: OwnerGrantBook,
    ) -> Self {
        Self::attached(principal, HarnessOwnerComposition::new(owner, grants))
    }

    #[must_use]
    pub fn with_owner_scope(
        principal: impl Into<String>,
        scope: SessionScopeView,
        owner: std::sync::Arc<dyn crate::owner::HarnessOwner>,
        grants: OwnerGrantBook,
    ) -> Self {
        Self::attached_with_scope(
            principal,
            scope,
            HarnessOwnerComposition::new(owner, grants),
        )
    }

    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.owner.is_some()
    }

    pub fn revoke_grant(&self, grant_id: &str) -> Result<(), crate::owner::OwnerAuthError> {
        self.owner
            .as_ref()
            .ok_or(crate::owner::OwnerAuthError::GrantBookUnavailable)?
            .revoke(grant_id)
    }

    /// Registers a binding returned by a harness owner so later path references can be checked
    /// locally before forwarding. This is useful when a composition is restored from a durable
    /// owner snapshot.
    pub fn register_attached_binding(
        &mut self,
        binding_id: impl Into<String>,
        scope: SessionScopeView,
    ) -> Result<(), SessionApiError> {
        let binding_id = binding_id.into();
        if !valid_id(&binding_id) {
            return Err(SessionApiError::BadRequest);
        }
        let expected_scope = match &self.attached_scope {
            Some(scope) => scope.clone(),
            None => scope_for_run(&scope.run_id),
        };
        if expected_scope != scope {
            return Err(SessionApiError::NotFound);
        }
        if !self.attached_bindings.contains_key(&binding_id)
            && self.attached_bindings.len() >= MAX_BINDINGS
        {
            return Err(SessionApiError::Capacity);
        }
        self.attached_bindings.insert(binding_id, scope);
        Ok(())
    }

    /// Registers an owner operation identity restored from the owner's durable receipt index.
    /// Foreign identities are rejected before any operation lookup is forwarded.
    pub fn register_attached_operation(
        &mut self,
        operation_id: impl Into<String>,
        scope: SessionScopeView,
    ) -> Result<(), SessionApiError> {
        let operation_id = operation_id.into();
        if !valid_id(&operation_id) {
            return Err(SessionApiError::BadRequest);
        }
        let expected_scope = match &self.attached_scope {
            Some(scope) => scope.clone(),
            None => scope_for_run(&scope.run_id),
        };
        if expected_scope != scope {
            return Err(SessionApiError::NotFound);
        }
        if !self.attached_operations.contains_key(&operation_id)
            && self.attached_operations.len() >= MAX_OPERATIONS
        {
            return Err(SessionApiError::Capacity);
        }
        self.attached_operations.insert(operation_id, scope);
        Ok(())
    }

    #[must_use]
    pub fn disabled(principal: impl Into<String>) -> Self {
        let mut route = Self::fixture(principal);
        route.mode = SessionRouteMode::Disabled;
        route
    }

    #[must_use]
    pub fn inspect_only(principal: impl Into<String>) -> Self {
        let mut route = Self::fixture(principal);
        route.mode = SessionRouteMode::InspectOnly;
        route
    }

    #[must_use]
    pub fn mode(&self) -> SessionRouteMode {
        self.mode.clone()
    }

    /// Select an explicit legacy advertisement for rollback; v1 discloses no executable limits.
    #[must_use]
    pub fn with_capability_version(mut self, version: crate::CapabilityVersion) -> Self {
        self.capability_version = version;
        self
    }

    #[must_use]
    pub fn consumer_pin(&self) -> crate::effective_limits::ConsumerPin {
        crate::effective_limits::console_consumer_pin_for(self.capability_version)
    }

    /// The local v3 descriptor. Attached routes obtain and authenticate an actual owner reply.
    #[must_use]
    pub fn capabilities(&self) -> SessionCapabilitiesView {
        self.capabilities.clone()
    }

    /// The local synthetic v3 descriptor, validated before the served route presents it.
    #[must_use]
    pub fn target_capabilities_v3(&self) -> &SessionCapabilitiesView {
        &self.capabilities
    }

    /// Derivation of the trusted effective-limit record from the `v3` target capability descriptor.
    #[must_use]
    pub fn effective_limit_record(&self) -> EffectiveLimitRecord {
        self.capabilities.effective_limit_record()
    }

    /// Admit a session policy value against the advertised executable ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when disabled, absent, or above the executable ceiling.
    pub fn admit_policy_value(&self, field: &str, requested: u64) -> Result<(), UnavailableReason> {
        self.capabilities.admit_policy_value(field, requested)
    }

    /// Admit a value only after authenticating the published record against the advertised
    /// descriptor's derivation.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when the record is stale, tampered, targets another profile,
    /// or the value exceeds the executable ceiling.
    pub fn admit_authorized_record(
        &self,
        record: &EffectiveLimitRecord,
        field: &str,
        requested: u64,
    ) -> Result<(), UnavailableReason> {
        self.capabilities
            .admit_authorized_record(record, field, requested)
    }

    pub fn handle(
        &mut self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if self.owner.is_some() {
            // The compatibility signature cannot carry Host, Origin or CSRF proofs. Attached
            // routes therefore require the explicit `handle_with_context` entry point.
            return Err(SessionApiError::Unauthorized);
        }
        self.handle_local(method, path, principal, body)
    }

    /// Handles an attached request after validating the complete origin/Host/CSRF and grant
    /// context. The old fixture route remains available through [`Self::fixture`].
    pub fn handle_with_context(
        &mut self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if self.owner.is_some() {
            self.handle_attached(method, path, context, body)
        } else {
            self.handle_local(method, path, &context.principal, body)
        }
    }

    fn handle_local(
        &mut self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if principal != self.principal || principal.is_empty() {
            return Err(SessionApiError::Unauthorized);
        }
        if body.len() > MAX_BODY_BYTES
            || path.contains("..")
            || path.contains('\\')
            || path.contains('%')
        {
            return Err(SessionApiError::BadRequest);
        }
        if method == "GET" && !body.is_empty() {
            return Err(SessionApiError::BadRequest);
        }
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.len() < 4
            || segments[0] != "v1"
            || segments[1] != "runs"
            || !matches!(
                segments[3],
                "provider-sessions" | "provider-session-operations" | "provider-session-events"
            )
        {
            return Err(SessionApiError::NotFound);
        }
        let run_id = segments[2];
        if !valid_id(run_id) {
            return Err(SessionApiError::BadRequest);
        }
        if self.mode == SessionRouteMode::Disabled {
            return Err(SessionApiError::Unsupported);
        }
        if self.mode == SessionRouteMode::InspectOnly && method != "GET" {
            return Err(SessionApiError::Unsupported);
        }
        if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "capabilities"
        {
            return if method == "GET" {
                self.capabilities
                    .validate_descriptor()
                    .map_err(SessionApiError::EffectiveLimit)?;
                let value = match self.capability_version {
                    crate::CapabilityVersion::V3 => serde_json::to_value(self.capabilities()),
                    crate::CapabilityVersion::V1 => serde_json::to_value(self.capabilities.to_v1()),
                }
                .map_err(|_| SessionApiError::BadRequest)?;
                Ok(self.envelope("capabilities", value))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 4 && segments[3] == "provider-sessions" {
            return if method == "GET" {
                let bindings = self
                    .bindings
                    .values()
                    .filter(|binding| binding.scope.run_id == run_id)
                    .collect::<Vec<_>>();
                let operations = self
                    .operations
                    .values()
                    .filter(|operation| operation.scope.run_id == run_id)
                    .collect::<Vec<_>>();
                Ok(self.envelope(
                    "list",
                    json!({"run_id":run_id,"bindings":bindings,"operations":operations,"next_cursor":null}),
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 5 && segments[3] == "provider-sessions" && segments[4] == "candidates"
        {
            return self.mutate_candidate(method, run_id, body);
        }
        if segments.len() == 4 && segments[3] == "provider-session-events" {
            return if method == "GET" {
                Ok(self.envelope(
                    "events",
                    json!({"run_id":run_id,"events":[],"next_cursor":null}),
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 5 && segments[3] == "provider-sessions" && segments[4] == "events" {
            return if method == "GET" {
                Ok(self.envelope(
                    "events",
                    json!({"run_id":run_id,"events":[],"next_cursor":null}),
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments[3] == "provider-session-operations" {
            if segments.len() != 5 || method != "GET" {
                return Err(SessionApiError::MethodNotAllowed);
            }
            let operation = self
                .operations
                .get(segments.get(4).copied().ok_or(SessionApiError::NotFound)?)
                .ok_or(SessionApiError::NotFound)?;
            if operation.scope.run_id != run_id {
                return Err(SessionApiError::NotFound);
            }
            return Ok(self.envelope(
                "operation",
                serde_json::to_value(operation).map_err(|_| SessionApiError::BadRequest)?,
            ));
        }
        if segments[3] != "provider-sessions" {
            return Err(SessionApiError::NotFound);
        }
        if segments.len() == 5 {
            let binding = self
                .bindings
                .get(segments[4])
                .ok_or(SessionApiError::NotFound)?;
            if binding.scope.run_id != run_id {
                return Err(SessionApiError::NotFound);
            }
            return if method == "GET" {
                Ok(self.envelope(
                    "binding",
                    serde_json::to_value(binding).map_err(|_| SessionApiError::BadRequest)?,
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        let binding_id = segments[4];
        let binding = self
            .bindings
            .get(binding_id)
            .ok_or(SessionApiError::NotFound)?
            .clone();
        if binding.scope.run_id != run_id {
            return Err(SessionApiError::NotFound);
        }
        if segments.len() == 6 && segments[5] == "history" {
            return if method == "GET" {
                Ok(self.envelope("history", json!({"schema":"ascension.provider-session.history.v1","view_id":format!("history-view-{binding_id}"),"binding_id":binding_id,"scope":binding.scope,"history_epoch":binding.history_epoch,"watermark":0,"coverage":binding.history_coverage,"effective_context_coverage":"unknown","items":[],"known_total_items":0,"next_cursor":null,"read_started_turn":false,"expires_at":binding.expires_at})))
            } else if method == "POST" {
                self.accept_operation(binding_id, "refresh", false, body)
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 6
            && matches!(
                segments[5],
                "fork-plans" | "compaction-plans" | "prepared-bindings"
            )
        {
            return if method == "POST" {
                self.accept_plan(binding_id, segments[5], body)
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 6 {
            let operation = match segments[5] {
                "reconnect" => "reconnect",
                "history-refresh" => "refresh",
                "fork-jobs" => "fork",
                "compaction-jobs" => "compact",
                "retire" => "retire",
                "cleanup" => "cleanup",
                _ => return Err(SessionApiError::NotFound),
            };
            return if method == "POST" {
                self.accept_operation(binding_id, operation, operation == "compact", body)
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        Err(SessionApiError::NotFound)
    }

    fn handle_attached(
        &mut self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if body.len() > MAX_BODY_BYTES
            || path.contains("..")
            || path.contains('\\')
            || path.contains('%')
        {
            return Err(SessionApiError::BadRequest);
        }
        if method == "GET" && !body.is_empty() {
            return Err(SessionApiError::BadRequest);
        }
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.len() < 4
            || segments[0] != "v1"
            || segments[1] != "runs"
            || !matches!(
                segments[3],
                "provider-sessions" | "provider-session-operations" | "provider-session-events"
            )
        {
            return Err(SessionApiError::NotFound);
        }
        let run_id = segments[2];
        if !valid_id(run_id) {
            return Err(SessionApiError::BadRequest);
        }
        let scope_view = match &self.attached_scope {
            Some(scope) if scope.run_id != run_id => return Err(SessionApiError::NotFound),
            Some(scope) => scope.clone(),
            None => scope_for_run(run_id),
        };
        let owner_scope = owner_scope(
            &scope_view.project_id,
            &scope_view.run_id,
            &scope_view.episode_id,
            &scope_view.agent_id,
        );
        let (operation, reference, write) = if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "capabilities"
        {
            (OwnerOperation::SessionCapabilities, None, false)
        } else if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "status"
        {
            (OwnerOperation::SessionStatus, None, false)
        } else if segments.len() == 4 && segments[3] == "provider-sessions" {
            (OwnerOperation::SessionList, None, false)
        } else if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "candidates"
        {
            (OwnerOperation::SessionCandidate, None, true)
        } else if (segments.len() == 4 && segments[3] == "provider-session-events")
            || (segments.len() == 5
                && segments[3] == "provider-sessions"
                && segments[4] == "events")
        {
            (OwnerOperation::SessionEvents, None, false)
        } else if segments.len() == 5 && segments[3] == "provider-session-operations" {
            let operation_id = segments[4];
            if !valid_id(operation_id)
                || self
                    .attached_operations
                    .get(operation_id)
                    .is_none_or(|scope| scope.run_id != run_id)
            {
                return Err(SessionApiError::NotFound);
            }
            (
                OwnerOperation::SessionOperation,
                Some(operation_id.to_owned()),
                false,
            )
        } else if segments[3] == "provider-sessions" && segments.len() >= 5 {
            let binding_id = segments[4];
            let Some(binding_scope) = self.attached_bindings.get(binding_id) else {
                return Err(SessionApiError::NotFound);
            };
            if binding_scope.run_id != run_id || !valid_id(binding_id) {
                return Err(SessionApiError::NotFound);
            }
            if segments.len() == 5 {
                if method != "GET" {
                    return Err(SessionApiError::MethodNotAllowed);
                }
                (
                    OwnerOperation::SessionBinding,
                    Some(binding_id.to_owned()),
                    false,
                )
            } else if segments.len() == 6 && segments[5] == "history" {
                if method == "GET" {
                    (
                        OwnerOperation::SessionHistory,
                        Some(binding_id.to_owned()),
                        false,
                    )
                } else if method == "POST" {
                    (
                        OwnerOperation::SessionHistoryRefresh,
                        Some(binding_id.to_owned()),
                        true,
                    )
                } else {
                    return Err(SessionApiError::MethodNotAllowed);
                }
            } else if segments.len() == 6 && segments[5] == "fork-plans" {
                (
                    OwnerOperation::SessionForkPlan,
                    Some(binding_id.to_owned()),
                    true,
                )
            } else if segments.len() == 6 && segments[5] == "compaction-plans" {
                (
                    OwnerOperation::SessionCompactionPlan,
                    Some(binding_id.to_owned()),
                    true,
                )
            } else if segments.len() == 6 && segments[5] == "prepared-bindings" {
                (
                    OwnerOperation::SessionPreparedBinding,
                    Some(binding_id.to_owned()),
                    true,
                )
            } else if segments.len() == 6 {
                let operation = match segments[5] {
                    "reconnect" => OwnerOperation::SessionReconnect,
                    "history-refresh" => OwnerOperation::SessionHistoryRefresh,
                    "fork-jobs" => OwnerOperation::SessionFork,
                    "compaction-jobs" => OwnerOperation::SessionCompaction,
                    "retire" => OwnerOperation::SessionRetire,
                    "cleanup" => OwnerOperation::SessionCleanup,
                    _ => return Err(SessionApiError::NotFound),
                };
                (operation, Some(binding_id.to_owned()), true)
            } else {
                return Err(SessionApiError::NotFound);
            }
        } else {
            return Err(SessionApiError::NotFound);
        };

        let payload = owner_session_payload(operation, method, body)?;
        self.delegate_attached(operation, owner_scope, reference, payload, context, write)
    }

    fn delegate_attached(
        &mut self,
        operation: OwnerOperation,
        scope: crate::owner::OwnerScope,
        reference: Option<String>,
        payload: Value,
        context: &OwnerRequestContext,
        write: bool,
    ) -> Result<Value, SessionApiError> {
        let composition = self.owner.as_ref().ok_or(SessionApiError::Unavailable)?;
        let grant = composition
            .authorize(context, operation.grant_class(), &scope, write)
            .map_err(|_| SessionApiError::Forbidden)?;
        self.ensure_attached_capacity(operation)?;
        let revocation_epoch = grant.revocation_epoch;
        let call = owner_call(operation, scope.clone(), reference.clone(), payload, grant)
            .map_err(|_| SessionApiError::BadRequest)?;
        let owner = composition.owner();
        let mut reply = match owner.call(call) {
            Ok(reply) => reply,
            Err(OwnerError::LostReply { receipt_id }) => {
                if !valid_id(&receipt_id) {
                    return Err(SessionApiError::Unavailable);
                }
                let lookup = crate::owner::OwnerReceiptLookup {
                    operation,
                    scope: scope.clone(),
                    reference,
                    receipt_id: receipt_id.clone(),
                };
                match owner.lookup_receipt(lookup) {
                    Ok(reply) => {
                        if reply.receipt.receipt_id != receipt_id {
                            return Err(SessionApiError::MalformedPeer);
                        }
                        reply
                    }
                    Err(OwnerError::UnknownReceipt | OwnerError::Unavailable) => {
                        crate::owner::OwnerReply::unknown(
                            receipt_id,
                            format!("unknown-{}", operation.as_str().replace('.', "-")),
                            operation,
                            "harness",
                            0,
                            "receipt_lookup_unavailable",
                        )
                        .map_err(|_| SessionApiError::MalformedPeer)?
                    }
                    Err(OwnerError::Unsupported) => crate::owner::OwnerReply::unsupported(
                        receipt_id.clone(),
                        "unsupported",
                        operation,
                        "harness",
                        0,
                        "owner_unsupported",
                    )
                    .map_err(|_| SessionApiError::MalformedPeer)?,
                    Err(OwnerError::LostReply { .. } | OwnerError::MalformedResponse) => {
                        return Err(SessionApiError::Unavailable);
                    }
                }
            }
            Err(OwnerError::Unsupported) => crate::owner::OwnerReply::unsupported(
                "unsupported-receipt",
                "unsupported",
                operation,
                "harness",
                0,
                "owner_unsupported",
            )
            .map_err(|_| SessionApiError::MalformedPeer)?,
            Err(OwnerError::Unavailable | OwnerError::UnknownReceipt) => {
                return Err(SessionApiError::Unavailable);
            }
            Err(OwnerError::MalformedResponse) => return Err(SessionApiError::MalformedPeer),
        };
        reply
            .validate_for(operation)
            .map_err(|_| SessionApiError::MalformedPeer)?;
        composition
            .capability_trust
            .present(operation, &scope, &mut reply, self.capability_version)
            .map_err(SessionApiError::EffectiveLimit)?;
        self.record_attached_references(&scope, reply.value.as_ref())?;
        owner_session_result(operation, revocation_epoch, reply)
    }

    fn ensure_attached_capacity(&self, operation: OwnerOperation) -> Result<(), SessionApiError> {
        let creates_binding = matches!(
            operation,
            OwnerOperation::SessionCandidate | OwnerOperation::SessionPreparedBinding
        );
        let creates_operation = matches!(
            operation,
            OwnerOperation::SessionCandidate
                | OwnerOperation::SessionHistoryRefresh
                | OwnerOperation::SessionReconnect
                | OwnerOperation::SessionForkPlan
                | OwnerOperation::SessionCompactionPlan
                | OwnerOperation::SessionPreparedBinding
                | OwnerOperation::SessionFork
                | OwnerOperation::SessionCompaction
                | OwnerOperation::SessionRetire
                | OwnerOperation::SessionCleanup
        );
        if creates_binding && self.attached_bindings.len() >= MAX_BINDINGS {
            return Err(SessionApiError::Capacity);
        }
        if creates_operation && self.attached_operations.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        Ok(())
    }

    fn record_attached_references(
        &mut self,
        scope: &crate::owner::OwnerScope,
        value: Option<&Value>,
    ) -> Result<(), SessionApiError> {
        let Some(object) = value.and_then(Value::as_object) else {
            return Ok(());
        };
        let binding_id = object
            .get("binding_id")
            .and_then(Value::as_str)
            .filter(|value| valid_id(value));
        if binding_id.is_some_and(|binding_id| {
            !self.attached_bindings.contains_key(binding_id)
                && self.attached_bindings.len() >= MAX_BINDINGS
        }) {
            return Err(SessionApiError::Capacity);
        }

        let mut new_operation_ids = Vec::new();
        for key in ["operation_id", "plan_id"] {
            if let Some(operation_id) = object
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| valid_id(value))
                && !self.attached_operations.contains_key(operation_id)
                && !new_operation_ids.iter().any(|known| known == operation_id)
            {
                new_operation_ids.push(operation_id.to_owned());
            }
        }
        if self
            .attached_operations
            .len()
            .saturating_add(new_operation_ids.len())
            > MAX_OPERATIONS
        {
            return Err(SessionApiError::Capacity);
        }

        if let Some(binding_id) = binding_id {
            self.attached_bindings.insert(
                binding_id.to_owned(),
                SessionScopeView {
                    project_id: scope.project_id.clone(),
                    run_id: scope.run_id.clone(),
                    episode_id: scope.episode_id.clone(),
                    agent_id: scope.agent_id.clone(),
                },
            );
        }
        for operation_id in new_operation_ids {
            self.attached_operations.insert(
                operation_id,
                SessionScopeView {
                    project_id: scope.project_id.clone(),
                    run_id: scope.run_id.clone(),
                    episode_id: scope.episode_id.clone(),
                    agent_id: scope.agent_id.clone(),
                },
            );
        }
        Ok(())
    }

    fn mutate_candidate(
        &mut self,
        method: &str,
        run_id: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if method != "POST" {
            return Err(SessionApiError::MethodNotAllowed);
        }
        if body.len() > MAX_BODY_BYTES {
            return Err(SessionApiError::Capacity);
        }
        let (value, idempotency_key) = parse_command(
            body,
            &[
                "idempotency_key",
                "expected_control_generation",
                "approved_policy_ref",
                "profile_ref",
                "purpose",
            ],
            &[
                "idempotency_key",
                "expected_control_generation",
                "approved_policy_ref",
                "profile_ref",
                "purpose",
            ],
        )?;
        let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
        require_u64(object, "expected_control_generation")?;
        require_id(object, "approved_policy_ref")?;
        require_id(object, "profile_ref")?;
        match object.get("purpose").and_then(Value::as_str) {
            Some("executable_candidate" | "evaluation") => {}
            _ => return Err(SessionApiError::BadRequest),
        }
        let request_sha256 = canonical_digest(&value);
        let dedupe_key = format!("candidate:{run_id}:{idempotency_key}");
        if let Some(existing) = self.idempotency.get(&dedupe_key) {
            if existing.request_sha256 != request_sha256 {
                return Err(SessionApiError::Conflict);
            }
            return Ok(self.accepted_operation(existing));
        }
        if self.bindings.len() >= MAX_BINDINGS {
            return Err(SessionApiError::Capacity);
        }
        if self
            .bindings
            .values()
            .filter(|binding| binding.scope.run_id == run_id)
            .filter(|binding| binding.state == "candidate" || binding.purpose == "evaluation")
            .count()
            >= MAX_CANDIDATES
        {
            return Err(SessionApiError::Capacity);
        }
        let binding_id = format!("binding-{}", self.next_id);
        let operation_id = format!("operation-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let purpose = if object
            .get("purpose")
            .and_then(Value::as_str)
            .is_some_and(|purpose| purpose == "evaluation")
        {
            "evaluation"
        } else {
            "executable"
        };
        let scope = scope_for_run(run_id);
        self.bindings.insert(
            binding_id.clone(),
            SessionBindingView {
                schema: "ascension.provider-session.binding.v1".to_owned(),
                binding_id: binding_id.clone(),
                scope: scope.clone(),
                branch_id: "branch-fixture".to_owned(),
                credential_realm_ref: "fixture-realm".to_owned(),
                profile_sha256: sha256_hex("codex-app-server-fixture-v1"),
                owner_epoch: 1,
                session_epoch: 1,
                run_id: run_id.to_owned(),
                state: "candidate".to_owned(),
                purpose: purpose.to_owned(),
                native_thread_ref: format!("pending-{binding_id}"),
                dependency_ids: Vec::new(),
                continuity_sha256: sha256_hex("empty-provider-history"),
                history_coverage: "unknown".to_owned(),
                history_epoch: 0,
                compaction_epoch: 0,
                dependency_count: 0,
                game_dispatch_capability: false,
                expires_at: fixture_expiry(),
            },
        );
        self.operations.insert(
            operation_id.clone(),
            SessionOperationView {
                schema: "ascension.provider-session.operation.v1".to_owned(),
                operation_id: operation_id.clone(),
                scope,
                binding_id: binding_id.clone(),
                kind: "create_candidate".to_owned(),
                idempotency_key: idempotency_key.clone(),
                request_sha256: request_sha256.clone(),
                state: "intent_persisted".to_owned(),
                owner_epoch: 1,
                session_epoch: 1,
                generation_permission: false,
                generation_class: false,
                automatic_retry: false,
                auto_resume: false,
                game_effects: 0,
                terminal_evidence_ref: None,
            },
        );
        self.idempotency.insert(
            dedupe_key,
            IdempotencyRecord {
                request_sha256,
                operation_id: operation_id.clone(),
                binding_id: binding_id.clone(),
            },
        );
        Ok(self.envelope("accepted", json!({"operation_id":operation_id,"binding_id":binding_id,"status":"intent_persisted","native_calls":0,"game_effects":0})))
    }

    fn accept_plan(
        &mut self,
        binding_id: &str,
        kind: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if body.len() > MAX_BODY_BYTES {
            return Err(SessionApiError::Capacity);
        }
        let (value, idempotency_key) = match kind {
            "fork-plans" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "cutoff_turn_ref",
                    "operation",
                    "purpose",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "cutoff_turn_ref",
                    "operation",
                    "purpose",
                ],
            )?,
            "compaction-plans" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "requested_policy",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "requested_policy",
                ],
            )?,
            "prepared-bindings" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "phase2_draft_ref",
                    "phase3_selection_ref",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "phase2_draft_ref",
                    "phase3_selection_ref",
                ],
            )?,
            _ => return Err(SessionApiError::Unsupported),
        };
        let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
        for field in [
            "expected_control_generation",
            "expected_session_epoch",
            "expected_history_epoch",
        ] {
            require_u64(object, field)?;
        }
        match kind {
            "fork-plans" => {
                require_id(object, "cutoff_turn_ref")?;
                if !matches!(
                    object.get("operation").and_then(Value::as_str),
                    Some("native_fork" | "clean_rehydration")
                ) || object.get("purpose").and_then(Value::as_str) != Some("evaluation")
                {
                    return Err(SessionApiError::BadRequest);
                }
            }
            "compaction-plans" => {
                if !matches!(
                    object.get("requested_policy").and_then(Value::as_str),
                    Some("strict_reviewed" | "observed_persistent")
                ) {
                    return Err(SessionApiError::BadRequest);
                }
            }
            "prepared-bindings" => {
                require_id(object, "phase2_draft_ref")?;
                require_id(object, "phase3_selection_ref")?;
            }
            _ => return Err(SessionApiError::Unsupported),
        }
        let request_sha256 = canonical_digest(&value);
        let dedupe_key = format!("plan:{kind}:{binding_id}:{idempotency_key}");
        if let Some(existing) = self.idempotency.get(&dedupe_key) {
            if existing.request_sha256 != request_sha256 {
                return Err(SessionApiError::Conflict);
            }
            return Ok(self.accepted_plan(existing, kind));
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        if self.idempotency.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        let plan_id = format!("plan-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        if matches!(kind, "fork-plans" | "compaction-plans") {
            let (scope, session_epoch) = self
                .bindings
                .get(binding_id)
                .map(|binding| (binding.scope.clone(), binding.session_epoch))
                .ok_or(SessionApiError::NotFound)?;
            let operation_kind = if kind == "fork-plans" {
                "fork"
            } else {
                "compact"
            };
            self.operations.insert(
                plan_id.clone(),
                SessionOperationView {
                    schema: "ascension.provider-session.operation.v1".to_owned(),
                    operation_id: plan_id.clone(),
                    scope,
                    binding_id: binding_id.to_owned(),
                    kind: operation_kind.to_owned(),
                    idempotency_key: idempotency_key.clone(),
                    request_sha256: request_sha256.clone(),
                    state: "planned".to_owned(),
                    owner_epoch: 1,
                    session_epoch,
                    generation_permission: kind == "compaction-plans",
                    generation_class: kind == "compaction-plans",
                    automatic_retry: false,
                    auto_resume: false,
                    game_effects: 0,
                    terminal_evidence_ref: None,
                },
            );
        }
        self.idempotency.insert(
            dedupe_key,
            IdempotencyRecord {
                request_sha256,
                operation_id: plan_id.clone(),
                binding_id: binding_id.to_owned(),
            },
        );
        Ok(self.envelope(
            "plan",
            json!({
                "plan_id": plan_id,
                "binding_id": binding_id,
                "kind": kind,
                "status": "planned",
                "inference_calls": 0,
                "game_effects": 0
            }),
        ))
    }

    fn accept_operation(
        &mut self,
        binding_id: &str,
        kind: &str,
        generation_permission: bool,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if body.len() > MAX_BODY_BYTES {
            return Err(SessionApiError::Capacity);
        }
        let (value, idempotency_key) = match kind {
            "refresh" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                ],
            )?,
            "reconnect" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                ],
            )?,
            "fork" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_fork_plan_ref",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_fork_plan_ref",
                ],
            )?,
            "compact" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_compaction_plan_ref",
                    "spend_authorization_ref",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_compaction_plan_ref",
                    "spend_authorization_ref",
                ],
            )?,
            "retire" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "reason",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "reason",
                ],
            )?,
            "cleanup" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "retirement_ref",
                    "erase_authorization_ref",
                    "expected_session_epoch",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "retirement_ref",
                    "erase_authorization_ref",
                    "expected_session_epoch",
                ],
            )?,
            _ => return Err(SessionApiError::Unsupported),
        };
        let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
        for field in [
            "expected_control_generation",
            "expected_session_epoch",
            "expected_history_epoch",
        ] {
            if object.contains_key(field) {
                require_u64(object, field)?;
            }
        }
        for field in [
            "approved_fork_plan_ref",
            "approved_compaction_plan_ref",
            "spend_authorization_ref",
            "retirement_ref",
            "erase_authorization_ref",
        ] {
            if object.contains_key(field) {
                require_id(object, field)?;
            }
        }
        if kind == "retire"
            && !matches!(
                object.get("reason").and_then(Value::as_str),
                Some(
                    "operator_request"
                        | "source_removed"
                        | "continuity_lost"
                        | "scope_ended"
                        | "capacity_rotation"
                )
            )
        {
            return Err(SessionApiError::BadRequest);
        }
        if kind == "compact" && !object.contains_key("spend_authorization_ref") {
            return Err(SessionApiError::BadRequest);
        }
        let request_sha256 = canonical_digest(&value);
        let dedupe_key = format!("{kind}:{binding_id}:{idempotency_key}");
        if let Some(existing) = self.idempotency.get(&dedupe_key) {
            if existing.request_sha256 != request_sha256 {
                return Err(SessionApiError::Conflict);
            }
            return Ok(self.accepted_operation(existing));
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        let scope = self
            .bindings
            .get(binding_id)
            .map(|binding| binding.scope.clone())
            .ok_or(SessionApiError::NotFound)?;
        let session_epoch = self
            .bindings
            .get(binding_id)
            .map_or(1, |binding| binding.session_epoch);
        let operation_id = format!("operation-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.operations.insert(
            operation_id.clone(),
            SessionOperationView {
                schema: "ascension.provider-session.operation.v1".to_owned(),
                operation_id: operation_id.clone(),
                scope,
                binding_id: binding_id.to_owned(),
                kind: kind.to_owned(),
                idempotency_key: idempotency_key.clone(),
                request_sha256: request_sha256.clone(),
                state: "intent_persisted".to_owned(),
                owner_epoch: 1,
                session_epoch,
                generation_permission,
                generation_class: generation_permission,
                automatic_retry: false,
                auto_resume: false,
                game_effects: 0,
                terminal_evidence_ref: None,
            },
        );
        self.idempotency.insert(
            dedupe_key,
            IdempotencyRecord {
                request_sha256,
                operation_id: operation_id.clone(),
                binding_id: binding_id.to_owned(),
            },
        );
        Ok(self.envelope("accepted", json!({"operation_id":operation_id,"binding_id":binding_id,"status":"intent_persisted","native_calls":0,"game_effects":0})))
    }

    fn accepted_operation(&self, record: &IdempotencyRecord) -> Value {
        self.envelope(
            "accepted",
            json!({
                "operation_id": record.operation_id,
                "binding_id": record.binding_id,
                "status": "intent_persisted",
                "native_calls": 0,
                "game_effects": 0
            }),
        )
    }

    fn accepted_plan(&self, record: &IdempotencyRecord, kind: &str) -> Value {
        self.envelope(
            "plan",
            json!({
                "plan_id": record.operation_id,
                "binding_id": record.binding_id,
                "kind": kind,
                "status": "planned",
                "inference_calls": 0,
                "game_effects": 0
            }),
        )
    }

    fn envelope(&self, operation: &str, value: Value) -> Value {
        json!({"schema":SESSION_API_SCHEMA,"operation":operation,"value":value,"effect_class":"local_metadata_only","inference_calls":0,"game_effects":0})
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

fn safe_generation(value: u64) -> bool {
    value <= MAX_SAFE_INTEGER
}

fn owner_session_payload(
    operation: OwnerOperation,
    method: &str,
    body: &[u8],
) -> Result<Value, SessionApiError> {
    if operation.read_only() {
        if method != "GET" || !body.is_empty() {
            return Err(if method == "GET" {
                SessionApiError::BadRequest
            } else {
                SessionApiError::MethodNotAllowed
            });
        }
        return Ok(json!({}));
    }
    if method != "POST" {
        return Err(SessionApiError::MethodNotAllowed);
    }
    match operation {
        OwnerOperation::SessionCandidate => {
            parse_typed_session_command(body, |command: &SessionCandidateCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.approved_policy_ref)
                    && valid_id(&command.profile_ref)
            })
        }
        OwnerOperation::SessionHistoryRefresh => {
            parse_typed_session_command(body, |command: &SessionHistoryRefreshCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
            })
        }
        OwnerOperation::SessionReconnect => {
            parse_typed_session_command(body, |command: &SessionReconnectCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
            })
        }
        OwnerOperation::SessionForkPlan => {
            parse_typed_session_command(body, |command: &SessionForkPlanCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
                    && valid_id(&command.cutoff_turn_ref)
            })
        }
        OwnerOperation::SessionCompactionPlan => {
            parse_typed_session_command(body, |command: &SessionCompactionPlanCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
            })
        }
        OwnerOperation::SessionPreparedBinding => {
            parse_typed_session_command(body, |command: &SessionPreparedBindingCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
                    && valid_id(&command.phase2_draft_ref)
                    && valid_id(&command.phase3_selection_ref)
            })
        }
        OwnerOperation::SessionFork => {
            parse_typed_session_command(body, |command: &SessionForkCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.approved_fork_plan_ref)
            })
        }
        OwnerOperation::SessionCompaction => {
            parse_typed_session_command(body, |command: &SessionCompactionCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.approved_compaction_plan_ref)
                    && valid_id(&command.spend_authorization_ref)
            })
        }
        OwnerOperation::SessionRetire => {
            parse_typed_session_command(body, |command: &SessionRetireCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
            })
        }
        OwnerOperation::SessionCleanup => {
            parse_typed_session_command(body, |command: &SessionCleanupCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.retirement_ref)
                    && valid_id(&command.erase_authorization_ref)
                    && safe_generation(command.expected_session_epoch)
            })
        }
        _ => Err(SessionApiError::Unsupported),
    }
}

fn parse_typed_session_command<T, F>(body: &[u8], validate: F) -> Result<Value, SessionApiError>
where
    T: serde::de::DeserializeOwned + Serialize,
    F: FnOnce(&T) -> bool,
{
    if body.is_empty() {
        return Err(SessionApiError::BadRequest);
    }
    let command: T = crate::parse_control_json(body).map_err(|_| SessionApiError::BadRequest)?;
    if !validate(&command) {
        return Err(SessionApiError::BadRequest);
    }
    let value = serde_json::to_value(command).map_err(|_| SessionApiError::BadRequest)?;
    validate_public_value(&value).map_err(|_| SessionApiError::BadRequest)?;
    Ok(value)
}

fn owner_session_result(
    operation: OwnerOperation,
    revocation_epoch: u64,
    reply: crate::owner::OwnerReply,
) -> Result<Value, SessionApiError> {
    let receipt =
        serde_json::to_value(&reply.receipt).map_err(|_| SessionApiError::MalformedPeer)?;
    let outcome = reply.receipt.outcome;
    let value = match reply.value {
        Some(value) => value,
        None => Value::Null,
    };
    Ok(json!({
        "schema": SESSION_API_SCHEMA,
        "operation": operation.as_str(),
        "source": reply.receipt.source,
        "owner_epoch": reply.receipt.owner_epoch,
        "revocation_epoch": revocation_epoch,
        "evidence": reply.receipt.evidence,
        "outcome": outcome.as_str(),
        "effect_class": if operation.read_only() {
            "local_read_no_inference"
        } else {
            "owner_delegated"
        },
        "receipt": receipt,
        "owner_receipt": receipt,
        "value": value,
    }))
}

fn scope_for_run(run_id: &str) -> SessionScopeView {
    SessionScopeView {
        project_id: "project-fixture".to_owned(),
        run_id: run_id.to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    }
}

fn fixture_expiry() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    crate::control::format_time(now.saturating_add(FIXTURE_EXPIRY_SECONDS))
}

fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    use sha2::{Digest as _, Sha256};
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn parse_command(
    body: &[u8],
    required: &[&str],
    allowed: &[&str],
) -> Result<(Value, String), SessionApiError> {
    if body.is_empty() {
        return Err(SessionApiError::BadRequest);
    }
    let value: Value = crate::parse_control_json(body).map_err(|_| SessionApiError::BadRequest)?;
    let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(SessionApiError::BadRequest);
    }
    if required.iter().any(|key| !object.contains_key(*key)) {
        return Err(SessionApiError::BadRequest);
    }
    let key = object
        .get("idempotency_key")
        .and_then(Value::as_str)
        .filter(|key| valid_id(key))
        .ok_or(SessionApiError::BadRequest)?
        .to_owned();
    Ok((value, key))
}

fn require_id(object: &serde_json::Map<String, Value>, field: &str) -> Result<(), SessionApiError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| valid_id(value))
        .map(|_| ())
        .ok_or(SessionApiError::BadRequest)
}

fn require_u64(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), SessionApiError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .map(|_| ())
        .ok_or(SessionApiError::BadRequest)
}

#[allow(clippy::manual_unwrap_or_default)]
fn canonical_digest(value: &Value) -> String {
    let canonical = canonical_value(value);
    let bytes = match serde_json::to_vec(&canonical) {
        Ok(bytes) => bytes,
        Err(_) => Vec::new(),
    };
    sha256_hex(bytes)
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut sorted = serde_json::Map::new();
            for (key, child) in entries {
                sorted.insert(key.clone(), canonical_value(child));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod capabilities_tests {
    use super::*;

    #[test]
    fn served_capability_is_valid_v3_with_explicit_v1_rollback() {
        let mut route = ProviderSessionRoute::fixture("operator");
        let served = route
            .handle(
                "GET",
                "/v1/runs/run-fixture/provider-sessions/capabilities",
                "operator",
                &[],
            )
            .expect("capabilities");
        assert_eq!(served["value"]["schema"], SESSION_CAPABILITIES_SCHEMA);
        route
            .capabilities()
            .validate_descriptor()
            .expect("valid descriptor");
        let mut route = route.with_capability_version(crate::CapabilityVersion::V1);
        let served = route
            .handle(
                "GET",
                "/v1/runs/run-fixture/provider-sessions/capabilities",
                "operator",
                &[],
            )
            .expect("rollback");
        assert_eq!(served["value"]["schema"], SESSION_CAPABILITIES_SCHEMA_V1);
        assert!(served["value"].get("effective_limits").is_none());
        assert!(served["value"].get("binding").is_none());
    }

    #[test]
    fn v3_target_capability_advertises_effective_limits_and_binding() {
        let route = ProviderSessionRoute::fixture("operator");
        let value = serde_json::to_value(route.target_capabilities_v3()).expect("capabilities");
        assert_eq!(value["schema"], SESSION_CAPABILITIES_SCHEMA);
        assert_eq!(
            value["effective_limits"]["policy_schema"],
            "ascension.provider-session.policy.v1"
        );
        assert_eq!(value["effective_limits"]["max_completed_turns"], 128);
        assert_eq!(value["binding"]["owner"], "sts2-harness");
        assert_eq!(
            value["binding"]["owner_revision"],
            "harness-provider-session-v3"
        );
        assert_eq!(
            value["binding"]["descriptor_sha256"],
            route
                .target_capabilities_v3()
                .descriptor_digest()
                .expect("payload digest")
        );
    }

    #[test]
    fn dual_reader_reads_legacy_and_current_payloads() {
        let route = ProviderSessionRoute::fixture("operator");
        let v3_bytes = serde_json::to_vec(route.target_capabilities_v3()).expect("v3 bytes");
        let v3 = read_advertised_session_capabilities(&v3_bytes).expect("v3 reads");
        assert_eq!(v3.schema(), SESSION_CAPABILITIES_SCHEMA);
        // Slash-containing method names are admitted by the widened v3 pattern.
        let value: Value = serde_json::from_slice(&v3_bytes).expect("value");
        assert!(
            value["enabled_methods"]
                .as_array()
                .expect("methods")
                .iter()
                .any(|method| method == "thread/read")
        );
        assert_eq!(
            v3.effective_limits().expect("limits").max_completed_turns,
            128
        );

        let mut legacy =
            serde_json::to_value(route.target_capabilities_v3()).expect("capabilities");
        let object = legacy.as_object_mut().expect("object");
        object.remove("effective_limits");
        object.remove("binding");
        object.insert(
            "schema".to_owned(),
            Value::String(SESSION_CAPABILITIES_SCHEMA_V1.to_owned()),
        );
        let v1_bytes = serde_json::to_vec(&legacy).expect("v1 bytes");
        let v1 = read_advertised_session_capabilities(&v1_bytes).expect("v1 reads");
        assert_eq!(v1.schema(), SESSION_CAPABILITIES_SCHEMA_V1);
        assert!(v1.effective_limits().is_none());
        // A v1 payload cannot advertise the executable ceiling.
        assert_eq!(
            read_advertised_session_capabilities(b"not json"),
            Err(SessionCapabilitiesReadError::Malformed)
        );
    }

    #[test]
    fn session_admission_fails_closed_and_authenticates_record() {
        let route = ProviderSessionRoute::fixture("operator");
        assert_eq!(route.admit_policy_value("max_completed_turns", 128), Ok(()));
        assert_eq!(
            route.admit_policy_value("max_completed_turns", 129),
            Err(UnavailableReason::EffectiveLimitExceeded)
        );
        assert_eq!(
            route.admit_policy_value("absent", 1),
            Err(UnavailableReason::FieldNotAdvertised)
        );

        let trusted = route.effective_limit_record();
        let mut tampered = trusted.clone();
        tampered.rows[0].policy_schema_ceiling = Some(128);
        assert_eq!(
            route.admit_authorized_record(&tampered, "max_completed_turns", 128),
            Err(UnavailableReason::DescriptorTampered)
        );
    }
}
