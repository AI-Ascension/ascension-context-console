// SPDX-License-Identifier: MIT

use super::support::*;
use super::*;

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
        Some(SESSION_CAPABILITIES_SCHEMA_V3) => {
            serde_json::from_value::<SessionCapabilitiesV3>(value)
                .map(|capabilities| AdvertisedSessionCapabilities::V3(Box::new(capabilities)))
                .map_err(|_| SessionCapabilitiesReadError::Malformed)
        }
        Some(SESSION_CAPABILITIES_SCHEMA) => {
            serde_json::from_value::<SessionCapabilitiesView>(value)
                .map(|capabilities| AdvertisedSessionCapabilities::V4(Box::new(capabilities)))
                .map_err(|_| SessionCapabilitiesReadError::Malformed)
        }
        _ => Err(SessionCapabilitiesReadError::UnknownSchema),
    }
}

/// Portable `policy.v1` ceilings for the two policy-backed provider-session values.
const SESSION_POLICY_SCHEMA_MAX_COMPLETED_TURNS: u64 = 1024;
const SESSION_POLICY_SCHEMA_MAX_HISTORY_TTL_SECONDS: u64 = 604_800;

/// Derivation of the trusted effective-limit record for a provider-session descriptor.
///
/// `v3` and `v4` descriptors differ only in the qualifier field's name (`evidence` vs
/// `provenance`) and the pinned `binding.owner_revision` const, neither of which participates in the
/// record. Deriving both through this one function keeps the ceiling table and the record identity
/// identical across the rename.
fn derive_session_limit_record(
    schema: &str,
    binding: &SessionBinding,
    enabled_methods: &[String],
    limits: &SessionEffectiveLimits,
) -> EffectiveLimitRecord {
    EffectiveLimitRecord {
        schema: EFFECTIVE_LIMIT_RECORD_SCHEMA.to_owned(),
        surface: "provider-session".to_owned(),
        owner: binding.owner.clone(),
        owner_revision: binding.owner_revision.clone(),
        capability_schema: schema.to_owned(),
        capability_descriptor_sha256: binding.descriptor_sha256.clone(),
        enabled: !enabled_methods.is_empty(),
        rows: session_limit_rows(limits),
    }
}

fn session_limit_rows(limits: &SessionEffectiveLimits) -> Vec<LimitRow> {
    vec![
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
    ]
}

impl SessionCapabilitiesView {
    /// The legacy `v1` advertisement derived from this `v4` target descriptor. The served route
    /// keeps using this shape until the coordinated cutover. `v1` has no qualifier field, so the
    /// `v4` `provenance` value is carried into the legacy `evidence` name.
    #[must_use]
    pub fn to_v1(&self) -> SessionCapabilitiesV1 {
        SessionCapabilitiesV1 {
            schema: SESSION_CAPABILITIES_SCHEMA_V1.to_owned(),
            profile_id: self.profile_id.clone(),
            profile_sha256: self.profile_sha256.clone(),
            native_version: self.native_version.clone(),
            native_binary_sha256: self.native_binary_sha256.clone(),
            native_schema_sha256: self.native_schema_sha256.clone(),
            evidence: self.provenance.clone(),
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
        derive_session_limit_record(
            &self.schema,
            &self.binding,
            &self.enabled_methods,
            &self.effective_limits,
        )
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

impl SessionCapabilitiesV3 {
    /// Lift a `v3` descriptor into the `v4` shape: `evidence` is renamed `provenance`, the schema
    /// const and `binding.owner_revision` const move to `v4`, and the descriptor digest is
    /// recomputed over the resulting `v4` serialization.
    ///
    /// This is the one direction in which the rename is lossless, because the two schemas carry the
    /// same fields under different names and the qualifier value set is unchanged. It is used to
    /// admit a still-valid `v3` owner reply through a `v4` trust descriptor while the ecosystem is
    /// mid-cutover; the reverse lift is not offered, because a `v4` descriptor claiming the `v3`
    /// `owner_revision` const would not satisfy either contract.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason::DescriptorTampered`] if the recomputed digest cannot be
    /// derived from the lifted descriptor.
    pub fn to_v4(&self) -> Result<SessionCapabilitiesView, UnavailableReason> {
        let mut lifted = SessionCapabilitiesView {
            schema: SESSION_CAPABILITIES_SCHEMA.to_owned(),
            profile_id: self.profile_id.clone(),
            profile_sha256: self.profile_sha256.clone(),
            native_version: self.native_version.clone(),
            native_binary_sha256: self.native_binary_sha256.clone(),
            native_schema_sha256: self.native_schema_sha256.clone(),
            provenance: self.evidence.clone(),
            transport: self.transport.clone(),
            enabled_methods: self.enabled_methods.clone(),
            hardening: self.hardening.clone(),
            effective_limits: self.effective_limits.clone(),
            binding: SessionBinding {
                owner: self.binding.owner.clone(),
                owner_revision: SESSION_OWNER_REVISION_V4.to_owned(),
                policy_schema_sha256: self.binding.policy_schema_sha256.clone(),
                model_revision: self.binding.model_revision.clone(),
                adapter_revision: self.binding.adapter_revision.clone(),
                adapter_revision_sha256: self.binding.adapter_revision_sha256.clone(),
                descriptor_sha256: String::new(),
            },
            strict_executable: self.strict_executable,
            experimental_api: self.experimental_api,
            unknown_methods: self.unknown_methods.clone(),
            raw_rpc: self.raw_rpc,
        };
        lifted.binding.descriptor_sha256 = lifted.descriptor_digest()?;
        Ok(lifted)
    }

    /// The legacy `v1` advertisement derived from this `v3` descriptor. `v1` has no qualifier
    /// field, so the `v3` `evidence` value is carried into the legacy `evidence` name.
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

    /// Derivation of the trusted effective-limit record, shared verbatim with the `v4` view.
    #[must_use]
    pub fn effective_limit_record(&self) -> EffectiveLimitRecord {
        derive_session_limit_record(
            &self.schema,
            &self.binding,
            &self.enabled_methods,
            &self.effective_limits,
        )
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

pub(super) fn fixture_session_effective_limits() -> SessionEffectiveLimits {
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

pub(super) fn fixture_session_binding() -> SessionBinding {
    SessionBinding {
        owner: "sts2-harness".to_owned(),
        owner_revision: SESSION_OWNER_REVISION_V4.to_owned(),
        policy_schema_sha256: contract_pins::SESSION_POLICY_SCHEMA_SHA256.to_owned(),
        model_revision: "fixture-peer-1".to_owned(),
        adapter_revision: "codex-app-server-fixture-v1".to_owned(),
        adapter_revision_sha256: sha256_hex("codex-app-server-fixture-v1"),
        descriptor_sha256: String::new(),
    }
}
