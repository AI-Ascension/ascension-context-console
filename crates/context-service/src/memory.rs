// SPDX-License-Identifier: MIT

//! Target-owned, bounded facade for the harness Phase 3 memory contracts.
//!
//! The target never writes the harness corpus or scheduler directly.  This facade validates the
//! operator-facing route shape and exposes capability/status metadata; policy and source bytes
//! remain harness-owned.  It is safe to use while the feature is disabled and has no provider,
//! game, process, URL, or filesystem resolver path.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::effective_limits::{
    EFFECTIVE_LIMIT_RECORD_SCHEMA, EffectiveLimitRecord, LimitRow, UnavailableReason, contract_pins,
};
use crate::owner::{
    HarnessOwnerComposition, OwnerError, OwnerGrantBook, OwnerOperation, OwnerRequestContext,
    owner_call, owner_scope, validate_public_value,
};

/// Advertised capability schema. The `v3` contract additionally requires the `effective_limits`
/// and `binding` objects. The `v1` payload shape remains readable through the dual reader.
pub const MEMORY_CAPABILITIES_SCHEMA: &str = "ascension.context-memory.capabilities.v3";
/// Legacy capability schema preserved for dual reading during migration.
pub const MEMORY_CAPABILITIES_SCHEMA_V1: &str = "ascension.context-memory.capabilities.v1";
pub const MEMORY_QUERY_SCHEMA: &str = "ascension.context-memory.query.v1";
pub const MAX_MEMORY_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_MEMORY_BODY_BYTES: usize = 16 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Portable `policy.v1` ceiling for `optional_byte_budget`; broader than the executable ceiling.
const MEMORY_POLICY_SCHEMA_MAX_OPTIONAL_BYTES: u64 = 65_536;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryScope {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

/// Canonical `effective_limits` object required by the `context-memory` capability `v3` schema.
///
/// The fixture values in [`MemoryRoute::capabilities`] are synthetic and equal to the portable
/// schema ceilings; they are not a claim about a live harness owner or profile.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEffectiveLimits {
    pub policy_schema: String,
    pub max_candidates: u64,
    pub max_results: u64,
    pub max_selected: u64,
    pub optional_byte_budget: u64,
    pub max_entries_per_run: u64,
    pub max_corpus_bytes: u64,
    pub max_source_bytes: u64,
    pub max_sources_per_job: u64,
    pub max_job_input_bytes: u64,
    pub max_summary_output_bytes: u64,
    pub max_query_bytes: u64,
    pub max_lineage_depth: u64,
    pub max_global_memory_bytes: u64,
    pub max_global_memory_jobs: u64,
    pub max_retention_resources: u64,
    pub max_retention_bytes: u64,
    pub max_cache_entries: u64,
    pub max_review_records: u64,
    pub max_memory_bindings: u64,
    pub max_usage_attempts: u64,
}

/// Owner/adapter identity required by the `context-memory` capability `v3` schema.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBinding {
    pub owner: String,
    pub owner_revision: String,
    pub policy_schema_sha256: String,
    pub model_revision: String,
    pub adapter_revision: String,
    pub adapter_revision_sha256: String,
    pub descriptor_sha256: String,
}

/// Advertised `v3` context-memory capability descriptor.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCapabilities {
    pub schema: String,
    pub product_phase: u8,
    pub scope: MemoryScope,
    pub enabled: bool,
    pub local_lexical_retrieval: String,
    pub extractive_compaction: String,
    pub abstractive_adapter: String,
    pub abstractive_live_verified: bool,
    pub per_decision_policy: String,
    pub phase2_approval_required: bool,
    pub persistent_provider_sessions: bool,
    pub provider_side_compaction: bool,
    pub semantic_vector_retrieval: String,
    pub hidden_reasoning_access: bool,
    pub direct_game_dispatch: bool,
    pub effective_limits: MemoryEffectiveLimits,
    pub binding: MemoryBinding,
    pub supported_operations: Vec<String>,
}

/// Legacy `v1` capability descriptor, still readable through the dual reader.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCapabilitiesV1 {
    pub schema: String,
    pub product_phase: u8,
    pub scope: MemoryScope,
    pub enabled: bool,
    pub local_lexical_retrieval: String,
    pub extractive_compaction: String,
    pub abstractive_adapter: String,
    pub abstractive_live_verified: bool,
    pub per_decision_policy: String,
    pub phase2_approval_required: bool,
    pub persistent_provider_sessions: bool,
    pub provider_side_compaction: bool,
    pub semantic_vector_retrieval: String,
    pub hidden_reasoning_access: bool,
    pub direct_game_dispatch: bool,
    pub supported_operations: Vec<String>,
}

/// Result of the context-memory dual reader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdvertisedMemoryCapabilities {
    V1(Box<MemoryCapabilitiesV1>),
    V3(Box<MemoryCapabilities>),
}

impl AdvertisedMemoryCapabilities {
    #[must_use]
    pub fn schema(&self) -> &str {
        match self {
            Self::V1(capabilities) => &capabilities.schema,
            Self::V3(capabilities) => &capabilities.schema,
        }
    }

    #[must_use]
    pub fn effective_limits(&self) -> Option<&MemoryEffectiveLimits> {
        match self {
            Self::V1(_) => None,
            Self::V3(capabilities) => Some(&capabilities.effective_limits),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        match self {
            Self::V1(capabilities) => capabilities.enabled,
            Self::V3(capabilities) => capabilities.enabled,
        }
    }
}

/// Why a capability descriptor could not be read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilitiesReadError {
    UnknownSchema,
    Malformed,
}

/// Dual reader: a valid `v1` payload still reads, and a `v3` payload reads with effective limits.
///
/// # Errors
///
/// Returns [`CapabilitiesReadError::UnknownSchema`] for an unrecognized schema and
/// [`CapabilitiesReadError::Malformed`] for a payload that does not match the named version.
pub fn read_advertised_memory_capabilities(
    bytes: &[u8],
) -> Result<AdvertisedMemoryCapabilities, CapabilitiesReadError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| CapabilitiesReadError::Malformed)?;
    match value.get("schema").and_then(Value::as_str) {
        Some(MEMORY_CAPABILITIES_SCHEMA_V1) => {
            serde_json::from_value::<MemoryCapabilitiesV1>(value)
                .map(|capabilities| AdvertisedMemoryCapabilities::V1(Box::new(capabilities)))
                .map_err(|_| CapabilitiesReadError::Malformed)
        }
        Some(MEMORY_CAPABILITIES_SCHEMA) => serde_json::from_value::<MemoryCapabilities>(value)
            .map(|capabilities| AdvertisedMemoryCapabilities::V3(Box::new(capabilities)))
            .map_err(|_| CapabilitiesReadError::Malformed),
        _ => Err(CapabilitiesReadError::UnknownSchema),
    }
}

impl MemoryCapabilities {
    /// Derivation of the trusted effective-limit record from this validated capability
    /// descriptor. The record-under-test must be authenticated against this derivation, never
    /// the other way around.
    #[must_use]
    pub fn effective_limit_record(&self) -> EffectiveLimitRecord {
        let limits = &self.effective_limits;
        EffectiveLimitRecord {
            schema: EFFECTIVE_LIMIT_RECORD_SCHEMA.to_owned(),
            surface: "context-memory".to_owned(),
            owner: self.binding.owner.clone(),
            owner_revision: self.binding.owner_revision.clone(),
            capability_schema: self.schema.clone(),
            capability_descriptor_sha256: self.binding.descriptor_sha256.clone(),
            enabled: self.enabled,
            rows: vec![
                LimitRow::policy(
                    "max_candidates",
                    limits.max_candidates,
                    limits.max_candidates,
                    limits.max_candidates,
                    "MemoryPolicy::validate_schema+validate_against_capabilities",
                ),
                LimitRow::policy(
                    "max_results",
                    limits.max_results,
                    limits.max_results,
                    limits.max_results,
                    "MemoryPolicy::validate_schema+validate_against_capabilities",
                ),
                LimitRow::policy(
                    "max_selected",
                    limits.max_selected,
                    limits.max_selected,
                    limits.max_selected,
                    "MemoryPolicy::validate_schema+validate_against_capabilities",
                ),
                LimitRow::policy(
                    "optional_byte_budget",
                    MEMORY_POLICY_SCHEMA_MAX_OPTIONAL_BYTES,
                    limits.optional_byte_budget,
                    limits.optional_byte_budget,
                    "MemoryPolicy::validate_schema+validate_against_capabilities",
                ),
                LimitRow::profile_selected(
                    "max_entries_per_run",
                    limits.max_entries_per_run,
                    limits.max_entries_per_run,
                    "MemoryCorpus::with_limits",
                ),
                LimitRow::profile_selected(
                    "max_corpus_bytes",
                    limits.max_corpus_bytes,
                    limits.max_corpus_bytes,
                    "MemoryCorpus::with_limits",
                ),
                LimitRow::runtime_guard(
                    "max_source_bytes",
                    limits.max_source_bytes,
                    limits.max_source_bytes,
                    "MemoryCorpus admission",
                ),
                LimitRow::runtime_guard(
                    "max_sources_per_job",
                    limits.max_sources_per_job,
                    limits.max_sources_per_job,
                    "SummaryJob admission",
                ),
                LimitRow::runtime_guard(
                    "max_job_input_bytes",
                    limits.max_job_input_bytes,
                    limits.max_job_input_bytes,
                    "SummaryJob budget admission",
                ),
                LimitRow::runtime_guard(
                    "max_summary_output_bytes",
                    limits.max_summary_output_bytes,
                    limits.max_summary_output_bytes,
                    "SummaryJob output admission",
                ),
                LimitRow::runtime_guard(
                    "max_query_bytes",
                    limits.max_query_bytes,
                    limits.max_query_bytes,
                    "MemoryQuery validation",
                ),
                LimitRow::runtime_guard(
                    "max_lineage_depth",
                    limits.max_lineage_depth,
                    limits.max_lineage_depth,
                    "MemoryEntry lineage admission",
                ),
                LimitRow::runtime_guard(
                    "max_global_memory_bytes",
                    limits.max_global_memory_bytes,
                    limits.max_global_memory_bytes,
                    "MemoryBudgetLedger",
                ),
                LimitRow::runtime_guard(
                    "max_global_memory_jobs",
                    limits.max_global_memory_jobs,
                    limits.max_global_memory_jobs,
                    "MemoryBudgetLedger",
                ),
                LimitRow::runtime_guard(
                    "max_retention_resources",
                    limits.max_retention_resources,
                    limits.max_retention_resources,
                    "RetentionInventory",
                ),
                LimitRow::runtime_guard(
                    "max_retention_bytes",
                    limits.max_retention_bytes,
                    limits.max_retention_bytes,
                    "RetentionInventory",
                ),
                LimitRow::runtime_guard(
                    "max_cache_entries",
                    limits.max_cache_entries,
                    limits.max_cache_entries,
                    "RetrievalCache",
                ),
                LimitRow::runtime_guard(
                    "max_review_records",
                    limits.max_review_records,
                    limits.max_review_records,
                    "ImmutableReviewLedger",
                ),
                LimitRow::runtime_guard(
                    "max_memory_bindings",
                    limits.max_memory_bindings,
                    limits.max_memory_bindings,
                    "AtomicBindingStore",
                ),
                LimitRow::runtime_guard(
                    "max_usage_attempts",
                    limits.max_usage_attempts,
                    limits.max_usage_attempts,
                    "UsageLedger",
                ),
            ],
        }
    }

    /// Admit a policy value against this descriptor's executable ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when disabled, absent, or above the executable ceiling.
    pub fn admit_policy_value(&self, field: &str, requested: u64) -> Result<(), UnavailableReason> {
        self.effective_limit_record().admit(field, requested)
    }

    /// Admit a policy value only after authenticating the published record against this trusted
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

fn fixture_effective_limits() -> MemoryEffectiveLimits {
    MemoryEffectiveLimits {
        policy_schema: "ascension.context-memory.policy.v1".to_owned(),
        max_candidates: 64,
        max_results: 16,
        max_selected: 32,
        optional_byte_budget: 8192,
        max_entries_per_run: 10_000,
        max_corpus_bytes: 268_435_456,
        max_source_bytes: 65_536,
        max_sources_per_job: 16,
        max_job_input_bytes: 65_536,
        max_summary_output_bytes: 8192,
        max_query_bytes: 4096,
        max_lineage_depth: 2,
        max_global_memory_bytes: 262_144,
        max_global_memory_jobs: 32,
        max_retention_resources: 512,
        max_retention_bytes: 268_435_456,
        max_cache_entries: 256,
        max_review_records: 512,
        max_memory_bindings: 256,
        max_usage_attempts: 1024,
    }
}

fn fixture_binding() -> MemoryBinding {
    MemoryBinding {
        owner: "sts2-harness".to_owned(),
        owner_revision: "harness-context-memory-v3".to_owned(),
        policy_schema_sha256: contract_pins::MEMORY_POLICY_SCHEMA_SHA256.to_owned(),
        model_revision: "not-applicable".to_owned(),
        adapter_revision: "harness-context-memory-v3".to_owned(),
        adapter_revision_sha256: contract_pins::MEMORY_CAPABILITIES_SCHEMA_SHA256.to_owned(),
        descriptor_sha256: contract_pins::MEMORY_CAPABILITIES_SCHEMA_SHA256.to_owned(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQueryRequest {
    pub schema: String,
    pub scope: MemoryScope,
    pub branch_id: String,
    pub query: String,
    pub cutoff: u64,
    pub corpus_generation: u64,
    pub ranker_version: String,
    pub limit: usize,
    pub max_candidates: usize,
    pub effect_class: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryGenerationCommand {
    idempotency_key: String,
    proposal_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MemoryReviewDecision {
    Admit,
    Reject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryReviewCommand {
    idempotency_key: String,
    proposal_id: String,
    decision: MemoryReviewDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemorySelectionCommand {
    idempotency_key: String,
    selection_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryRouteError {
    MethodNotAllowed,
    BodyTooLarge,
    InvalidRequest,
    PermissionDenied,
    Unsupported,
    Unavailable,
}

impl std::fmt::Display for MemoryRouteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MethodNotAllowed => "memory route method is not allowed",
            Self::BodyTooLarge => "memory route body exceeds its bound",
            Self::InvalidRequest => "memory route request is invalid",
            Self::PermissionDenied => "memory route permission is denied",
            Self::Unsupported => "memory route operation is unsupported",
            Self::Unavailable => "memory route owner is unavailable",
        })
    }
}

impl std::error::Error for MemoryRouteError {}

#[derive(Clone, Debug)]
pub struct MemoryRoute {
    scope: MemoryScope,
    enabled: bool,
    search_principals: Vec<String>,
    review_principals: Vec<String>,
    owner: Option<HarnessOwnerComposition>,
}

impl PartialEq for MemoryRoute {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.enabled == other.enabled
            && self.search_principals == other.search_principals
            && self.review_principals == other.review_principals
            && self.owner.is_some() == other.owner.is_some()
    }
}

impl Eq for MemoryRoute {}

impl MemoryRoute {
    pub fn new(scope: MemoryScope, enabled: bool) -> Self {
        Self {
            scope,
            enabled,
            search_principals: Vec::new(),
            review_principals: Vec::new(),
            owner: None,
        }
    }

    /// Builds the explicitly attached composition. No local corpus or generation state is
    /// created; every supported operation is forwarded to `owner`.
    #[must_use]
    pub fn attached(scope: MemoryScope, composition: HarnessOwnerComposition) -> Self {
        Self {
            scope,
            enabled: true,
            search_principals: Vec::new(),
            review_principals: Vec::new(),
            owner: Some(composition),
        }
    }

    /// Convenience constructor for integrations that do not need to retain the composition.
    #[must_use]
    pub fn with_owner(
        scope: MemoryScope,
        owner: std::sync::Arc<dyn crate::owner::HarnessOwner>,
        grants: OwnerGrantBook,
    ) -> Self {
        Self::attached(scope, HarnessOwnerComposition::new(owner, grants))
    }

    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.owner.is_some()
    }

    /// Revokes a configured attached grant for all routes sharing its composition.
    pub fn revoke_grant(&self, grant_id: &str) -> Result<(), crate::owner::OwnerAuthError> {
        self.owner
            .as_ref()
            .ok_or(crate::owner::OwnerAuthError::GrantBookUnavailable)?
            .revoke(grant_id)
    }

    pub fn grant_search(&mut self, principal: impl Into<String>) {
        let principal = principal.into();
        if !principal.is_empty() && !self.search_principals.contains(&principal) {
            self.search_principals.push(principal);
        }
    }

    pub fn grant_review(&mut self, principal: impl Into<String>) {
        let principal = principal.into();
        if !principal.is_empty() && !self.review_principals.contains(&principal) {
            self.review_principals.push(principal);
        }
    }

    pub fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities {
            schema: MEMORY_CAPABILITIES_SCHEMA.to_owned(),
            product_phase: 3,
            scope: self.scope.clone(),
            enabled: self.enabled,
            // The target facade has no attached harness corpus or summary adapter.  Reporting
            // these lanes as supported would make an unavailable projection look like a working
            // product capability.  Keep the endpoint discoverable while naming the attachment
            // boundary explicitly.
            local_lexical_retrieval: "unverified".to_owned(),
            extractive_compaction: "unverified".to_owned(),
            abstractive_adapter: "unsupported".to_owned(),
            abstractive_live_verified: false,
            per_decision_policy: "unverified".to_owned(),
            phase2_approval_required: true,
            persistent_provider_sessions: false,
            provider_side_compaction: false,
            semantic_vector_retrieval: "unsupported".to_owned(),
            hidden_reasoning_access: false,
            direct_game_dispatch: false,
            effective_limits: fixture_effective_limits(),
            binding: fixture_binding(),
            supported_operations: if self.enabled {
                ["search"].into_iter().map(str::to_owned).collect()
            } else {
                Vec::new()
            },
        }
    }

    /// Derivation of the trusted effective-limit record from this validated capability
    /// descriptor. The record-under-test must be authenticated against this derivation, never
    /// the other way around.
    #[must_use]
    pub fn effective_limit_record(&self) -> EffectiveLimitRecord {
        let capabilities = self.capabilities();
        capabilities.effective_limit_record()
    }

    /// Admit a policy value against this descriptor's executable ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when disabled, absent, or above the executable ceiling.
    pub fn admit_policy_value(&self, field: &str, requested: u64) -> Result<(), UnavailableReason> {
        self.effective_limit_record().admit(field, requested)
    }

    /// Admit a policy value only after authenticating the published record against this trusted
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

    pub fn handle(
        &self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if self.owner.is_some() {
            // An attached route needs the caller's complete proof. The compatibility signature
            // has no Host, Origin or CSRF fields and must never reconstruct them from a grant.
            return Err(MemoryRouteError::PermissionDenied);
        }
        self.handle_local(method, path, principal, body)
    }

    /// Handles an attached request with the complete security context. Host/origin/CSRF and grant
    /// checks happen before constructing an owner call.
    pub fn handle_with_context(
        &self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if self.owner.is_some() {
            self.handle_attached(method, path, context, body)
        } else {
            self.handle_local(method, path, &context.principal, body)
        }
    }

    fn handle_local(
        &self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if body.len() > MAX_MEMORY_BODY_BYTES {
            return Err(MemoryRouteError::BodyTooLarge);
        }
        match (method, path) {
            ("GET", "/v3/memory/capabilities") => {
                if !self
                    .search_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                serde_json::to_value(self.capabilities()).map_err(|_| MemoryRouteError::Unsupported)
            }
            ("GET", "/v3/memory/status") => {
                if !self
                    .search_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                Ok(json!({
                    "schema": "ascension.context-memory.status.v1",
                    "enabled": self.enabled,
                    "corpus_generation": Value::Null,
                    "projection_generation": Value::Null,
                    "revocation_epoch": Value::Null,
                    "inference_calls": 0,
                    "effect_class": "local_read_no_inference"
                }))
            }
            ("POST", "/v3/memory/search") => {
                if !self
                    .search_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                let request: MemoryQueryRequest = crate::parse_control_json(body)
                    .map_err(|_| MemoryRouteError::InvalidRequest)?;
                if request.schema != MEMORY_QUERY_SCHEMA
                    || request.scope != self.scope
                    || !valid_id(&request.branch_id)
                    || request.query.is_empty()
                    || request.query.len() > MAX_MEMORY_QUERY_BYTES
                    || request.cutoff > MAX_SAFE_INTEGER
                    || request.corpus_generation == 0
                    || request.corpus_generation > MAX_SAFE_INTEGER
                    || !valid_id(&request.ranker_version)
                    || request.limit == 0
                    || request.limit > 16
                    || request.max_candidates == 0
                    || request.max_candidates > 64
                    || request.effect_class != "local_read_no_inference"
                {
                    return Err(MemoryRouteError::InvalidRequest);
                }
                if !self.enabled {
                    return Ok(json!({
                        "schema": "ascension.context-memory.retrieval.v1",
                        "query_id": "disabled",
                        "scope": self.scope,
                        "branch_id": request.branch_id,
                        "query_sha256": digest(request.query.as_bytes()),
                        "cutoff": request.cutoff,
                        "corpus_generation": request.corpus_generation,
                        "projection_generation": Value::Null,
                        "revocation_epoch": Value::Null,
                        "ranker_version": request.ranker_version,
                        "results": [],
                        "coverage": "projection_unavailable",
                        "inference_calls": 0
                    }));
                }
                Ok(json!({
                    "schema": "ascension.context-memory.retrieval.v1",
                    "query_id": "pending-harness-query",
                    "scope": self.scope,
                    "branch_id": request.branch_id,
                    "query_sha256": digest(request.query.as_bytes()),
                    "cutoff": request.cutoff,
                    "corpus_generation": request.corpus_generation,
                    "projection_generation": 0,
                    "revocation_epoch": 0,
                    "ranker_version": request.ranker_version,
                    "results": [],
                    "coverage": "projection_unavailable",
                    "inference_calls": 0
                }))
            }
            ("POST", "/v3/memory/generate") | ("POST", "/v3/memory/review") => {
                if !self
                    .review_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                Err(MemoryRouteError::Unsupported)
            }
            ("GET", _) | ("POST", _) => Err(MemoryRouteError::Unsupported),
            _ => Err(MemoryRouteError::MethodNotAllowed),
        }
    }

    fn handle_attached(
        &self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if body.len() > MAX_MEMORY_BODY_BYTES {
            return Err(MemoryRouteError::BodyTooLarge);
        }
        let operation = memory_operation(method, path, &self.scope.run_id)?;
        let write = !operation.read_only();
        let scope = owner_scope(
            &self.scope.project_id,
            &self.scope.run_id,
            &self.scope.episode_id,
            &self.scope.agent_id,
        );
        let composition = self.owner.as_ref().ok_or(MemoryRouteError::Unsupported)?;
        let grant = composition
            .authorize(context, operation.grant_class(), &scope, write)
            .map_err(|_| MemoryRouteError::PermissionDenied)?;
        let revocation_epoch = grant.revocation_epoch;
        let payload = match operation {
            OwnerOperation::MemoryCapabilities | OwnerOperation::MemoryStatus => {
                if !body.is_empty() {
                    return Err(MemoryRouteError::InvalidRequest);
                }
                json!({})
            }
            OwnerOperation::MemoryQuery => {
                let request: MemoryQueryRequest = crate::parse_control_json(body)
                    .map_err(|_| MemoryRouteError::InvalidRequest)?;
                validate_query(&request, &self.scope)?;
                serde_json::to_value(request).map_err(|_| MemoryRouteError::InvalidRequest)?
            }
            OwnerOperation::MemoryGeneration
            | OwnerOperation::MemoryReview
            | OwnerOperation::MemorySelection => parse_owner_command(operation, body)?,
            _ => return Err(MemoryRouteError::Unsupported),
        };
        let call = owner_call(operation, scope.clone(), None, payload, grant)
            .map_err(|_| MemoryRouteError::InvalidRequest)?;
        let owner = composition.owner();
        let reply = match owner.call(call) {
            Ok(reply) => reply,
            Err(OwnerError::LostReply { receipt_id }) => {
                if !valid_id(&receipt_id) {
                    return Err(MemoryRouteError::Unavailable);
                }
                let lookup = crate::owner::OwnerReceiptLookup {
                    operation,
                    scope,
                    reference: None,
                    receipt_id: receipt_id.clone(),
                };
                match owner.lookup_receipt(lookup) {
                    Ok(reply) => {
                        if reply.receipt.receipt_id != receipt_id {
                            return Err(MemoryRouteError::Unsupported);
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
                        .map_err(|_| MemoryRouteError::Unsupported)?
                    }
                    Err(OwnerError::Unsupported) => crate::owner::OwnerReply::unsupported(
                        receipt_id.clone(),
                        "unsupported",
                        operation,
                        "harness",
                        0,
                        "owner_unsupported",
                    )
                    .map_err(|_| MemoryRouteError::Unsupported)?,
                    Err(OwnerError::LostReply { .. } | OwnerError::MalformedResponse) => {
                        return Err(MemoryRouteError::Unsupported);
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
            .map_err(|_| MemoryRouteError::Unsupported)?,
            Err(OwnerError::Unavailable | OwnerError::UnknownReceipt) => {
                return Err(MemoryRouteError::Unavailable);
            }
            Err(OwnerError::MalformedResponse) => return Err(MemoryRouteError::Unsupported),
        };
        reply
            .validate_for(operation)
            .map_err(|_| MemoryRouteError::Unsupported)?;
        owner_result(operation, revocation_epoch, reply)
    }
}

fn validate_query(
    request: &MemoryQueryRequest,
    scope: &MemoryScope,
) -> Result<(), MemoryRouteError> {
    if request.schema != MEMORY_QUERY_SCHEMA
        || request.scope != *scope
        || !valid_id(&request.branch_id)
        || request.query.is_empty()
        || request.query.len() > MAX_MEMORY_QUERY_BYTES
        || request.cutoff > MAX_SAFE_INTEGER
        || request.corpus_generation == 0
        || request.corpus_generation > MAX_SAFE_INTEGER
        || !valid_id(&request.ranker_version)
        || request.limit == 0
        || request.limit > 16
        || request.max_candidates == 0
        || request.max_candidates > 64
        || request.effect_class != "local_read_no_inference"
    {
        return Err(MemoryRouteError::InvalidRequest);
    }
    Ok(())
}

fn memory_operation(
    method: &str,
    path: &str,
    expected_run_id: &str,
) -> Result<OwnerOperation, MemoryRouteError> {
    let operation = match (method, path) {
        ("GET", "/v3/memory/capabilities") => Some(OwnerOperation::MemoryCapabilities),
        ("GET", "/v3/memory/status") => Some(OwnerOperation::MemoryStatus),
        ("POST", "/v3/memory/search") => Some(OwnerOperation::MemoryQuery),
        ("POST", "/v3/memory/generate") => Some(OwnerOperation::MemoryGeneration),
        ("POST", "/v3/memory/review") => Some(OwnerOperation::MemoryReview),
        ("POST", "/v3/memory/select" | "/v3/memory/selection") => {
            Some(OwnerOperation::MemorySelection)
        }
        _ => None,
    };
    if let Some(operation) = operation {
        return Ok(operation);
    }
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() == 5
        && segments[0] == "v3"
        && segments[1] == "runs"
        && segments[3] == "context-memory"
        && segments[2] != expected_run_id
    {
        return Err(MemoryRouteError::PermissionDenied);
    }
    if segments.len() != 5
        || segments[0] != "v3"
        || segments[1] != "runs"
        || segments[2] != expected_run_id
        || segments[3] != "context-memory"
    {
        return Err(if method == "GET" {
            MemoryRouteError::Unsupported
        } else {
            MemoryRouteError::MethodNotAllowed
        });
    }
    match (method, segments[4]) {
        ("GET", "capabilities" | "status") => Ok(if segments[4] == "capabilities" {
            OwnerOperation::MemoryCapabilities
        } else {
            OwnerOperation::MemoryStatus
        }),
        ("POST", "search") => Ok(OwnerOperation::MemoryQuery),
        ("POST", "summary-jobs") => Ok(OwnerOperation::MemoryGeneration),
        ("POST", "reviews") => Ok(OwnerOperation::MemoryReview),
        ("POST", "selections") => Ok(OwnerOperation::MemorySelection),
        ("GET", _) => Err(MemoryRouteError::Unsupported),
        _ => Err(MemoryRouteError::MethodNotAllowed),
    }
}

fn parse_owner_command(operation: OwnerOperation, body: &[u8]) -> Result<Value, MemoryRouteError> {
    if body.is_empty() {
        return Err(MemoryRouteError::InvalidRequest);
    }
    match operation {
        OwnerOperation::MemoryGeneration => {
            parse_typed_owner_command(body, |command: &MemoryGenerationCommand| {
                valid_id(&command.idempotency_key) && valid_id(&command.proposal_id)
            })
        }
        OwnerOperation::MemoryReview => {
            parse_typed_owner_command(body, |command: &MemoryReviewCommand| {
                valid_id(&command.idempotency_key) && valid_id(&command.proposal_id)
            })
        }
        OwnerOperation::MemorySelection => {
            parse_typed_owner_command(body, |command: &MemorySelectionCommand| {
                valid_id(&command.idempotency_key) && valid_id(&command.selection_id)
            })
        }
        _ => Err(MemoryRouteError::Unsupported),
    }
}

fn parse_typed_owner_command<T, F>(body: &[u8], validate: F) -> Result<Value, MemoryRouteError>
where
    T: serde::de::DeserializeOwned + Serialize,
    F: FnOnce(&T) -> bool,
{
    let command: T =
        crate::parse_control_json(body).map_err(|_| MemoryRouteError::InvalidRequest)?;
    if !validate(&command) {
        return Err(MemoryRouteError::InvalidRequest);
    }
    let value = serde_json::to_value(command).map_err(|_| MemoryRouteError::InvalidRequest)?;
    validate_public_value(&value).map_err(|_| MemoryRouteError::InvalidRequest)?;
    Ok(value)
}

fn owner_result(
    operation: OwnerOperation,
    revocation_epoch: u64,
    reply: crate::owner::OwnerReply,
) -> Result<Value, MemoryRouteError> {
    let receipt =
        serde_json::to_value(&reply.receipt).map_err(|_| MemoryRouteError::Unsupported)?;
    let outcome = reply.receipt.outcome;
    let value = match reply.value {
        Some(value) => value,
        None => Value::Null,
    };
    Ok(json!({
        "schema": "ascension.context-memory.owner-result.v1",
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
        "value": value,
    }))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> MemoryScope {
        MemoryScope {
            project_id: "project".to_owned(),
            run_id: "run".to_owned(),
            episode_id: "episode".to_owned(),
            agent_id: "agent".to_owned(),
        }
    }

    #[test]
    fn disabled_capability_is_explicit_and_search_has_no_inference() {
        let mut route = MemoryRoute::new(scope(), false);
        route.grant_search("operator");
        assert_eq!(
            route.capabilities().supported_operations,
            Vec::<String>::new()
        );
        let request = serde_json::json!({
            "schema": MEMORY_QUERY_SCHEMA,
            "scope": scope(),
            "branch_id": "branch-a",
            "query": "HP settled",
            "cutoff": 10,
            "corpus_generation": 1,
            "ranker_version": "lexical-v1",
            "limit": 8,
            "max_candidates": 64,
            "effect_class": "local_read_no_inference"
        });
        let response = route
            .handle(
                "POST",
                "/v3/memory/search",
                "operator",
                &serde_json::to_vec(&request).unwrap_or_default(),
            )
            .unwrap_or(Value::Null);
        assert_eq!(response["inference_calls"], 0);
        assert_eq!(response["coverage"], "projection_unavailable");
    }

    #[test]
    fn generate_requires_separate_review_permission() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        assert_eq!(
            route.handle("POST", "/v3/memory/generate", "searcher", b"{}"),
            Err(MemoryRouteError::PermissionDenied)
        );
    }

    #[test]
    fn enabled_facade_does_not_advertise_unattached_lanes() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        let capabilities = route.capabilities();
        assert_eq!(capabilities.supported_operations, vec!["search"]);
        assert_eq!(capabilities.local_lexical_retrieval, "unverified");
        assert_eq!(capabilities.abstractive_adapter, "unsupported");
        assert_eq!(capabilities.per_decision_policy, "unverified");
        let status = route
            .handle("GET", "/v3/memory/status", "searcher", &[])
            .unwrap_or(Value::Null);
        assert!(status["projection_generation"].is_null());
        assert!(status["corpus_generation"].is_null());
    }

    #[test]
    fn v3_capability_advertises_effective_limits_and_binding() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        let value = serde_json::to_value(route.capabilities()).expect("capabilities");
        assert_eq!(value["schema"], MEMORY_CAPABILITIES_SCHEMA);
        assert_eq!(
            value["effective_limits"]["policy_schema"],
            "ascension.context-memory.policy.v1"
        );
        assert_eq!(value["effective_limits"]["max_candidates"], 64);
        assert_eq!(value["binding"]["owner"], "sts2-harness");
        assert_eq!(
            value["binding"]["owner_revision"],
            "harness-context-memory-v3"
        );
        assert_eq!(
            value["binding"]["descriptor_sha256"],
            contract_pins::MEMORY_CAPABILITIES_SCHEMA_SHA256
        );
    }

    #[test]
    fn dual_reader_reads_legacy_and_current_payloads() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        let v3_bytes = serde_json::to_vec(&route.capabilities()).expect("v3 bytes");
        let v3 = read_advertised_memory_capabilities(&v3_bytes).expect("v3 reads");
        assert_eq!(v3.schema(), MEMORY_CAPABILITIES_SCHEMA);
        let limits = v3.effective_limits().expect("v3 effective limits");
        assert_eq!(limits.max_results, 16);
        assert!(v3.enabled());

        let mut legacy = serde_json::to_value(route.capabilities()).expect("capabilities");
        let object = legacy.as_object_mut().expect("object");
        object.remove("effective_limits");
        object.remove("binding");
        object.insert(
            "schema".to_owned(),
            Value::String(MEMORY_CAPABILITIES_SCHEMA_V1.to_owned()),
        );
        let v1_bytes = serde_json::to_vec(&legacy).expect("v1 bytes");
        let v1 = read_advertised_memory_capabilities(&v1_bytes).expect("v1 reads");
        assert_eq!(v1.schema(), MEMORY_CAPABILITIES_SCHEMA_V1);
        assert!(v1.effective_limits().is_none());
        assert!(v1.enabled());
    }

    #[test]
    fn memory_admission_fails_closed_and_authenticates_record() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        assert_eq!(route.admit_policy_value("max_candidates", 64), Ok(()));
        assert_eq!(
            route.admit_policy_value("max_candidates", 65),
            Err(UnavailableReason::EffectiveLimitExceeded)
        );
        assert_eq!(
            route.admit_policy_value("absent", 1),
            Err(UnavailableReason::FieldNotAdvertised)
        );

        let trusted = route.effective_limit_record();
        assert_eq!(
            route.admit_authorized_record(&trusted, "max_results", 16),
            Ok(())
        );
        let mut tampered = trusted.clone();
        tampered.rows[0].executable_ceiling = 128;
        assert_eq!(
            route.admit_authorized_record(&tampered, "max_candidates", 64),
            Err(UnavailableReason::DescriptorTampered)
        );
    }
}
