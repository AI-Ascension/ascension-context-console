// SPDX-License-Identifier: MIT

//! Consumer-side effective-limit admission for the harness classification record.
//!
//! A portable JSON Schema ceiling is a syntax bound, not an execution promise; a field absent from
//! the harness record is unavailable, never unlimited. This module mirrors the producer contract
//! (`ascension.harness.effective-limits.v1`) so the console can authenticate a published record
//! against the derivation of the validated trusted capability descriptor before it presents any
//! value as supported.
//!
//! All behavior here is contract/fixture-level. The copied producer bytes are pinned by digest in
//! [`contract_pins`]; no native, provider, owner, or deployment behavior is claimed.
//!
//! Producer-generated synthetic conformance vectors check the surface-specific
//! `class`/`validator`/ceiling mapping against the pinned harness library, including restricted
//! profiles whose executable limits differ from schema maxima. Authentication still requires a
//! separately trusted capability descriptor; parsing a descriptor is not owner authentication.
//! These tests do not establish attached-owner, provider, native, or deployment acceptance.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Shared record schema for both the context-memory and provider-session surfaces.
pub const EFFECTIVE_LIMIT_RECORD_SCHEMA: &str = "ascension.harness.effective-limits.v1";

/// Repository identity this console records as the consumer pin holder.
pub const CONSUMER_REPOSITORY: &str = "AI-Ascension/ascension-context-console";

/// Harness producer revision whose copied artifacts are pinned by this repository.
pub const PRODUCER_REVISION: &str = "f8015e52ccb530e60d722283ef2b063da372169b";

/// How a published ceiling relates to the portable policy-schema ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitClass {
    /// The portable policy-schema ceiling and this profile's executable ceiling are identical.
    SchemaExecutableEqual,
    /// The portable policy schema intentionally admits larger values than this profile executes.
    SchemaBroaderThanExecutable,
    /// The ceiling is selected per corpus/profile and published as the executable ceiling.
    ProfileSelected,
    /// A runtime or transport guard with no portable policy field.
    RuntimeGuard,
}

impl LimitClass {
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::SchemaExecutableEqual => "schema_executable_equal",
            Self::SchemaBroaderThanExecutable => "schema_broader_than_executable",
            Self::ProfileSelected => "profile_selected",
            Self::RuntimeGuard => "runtime_guard",
        }
    }
}

/// Machine-readable reason a value is not executable on the selected owner/profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The value is schema-valid for this surface but exceeds the executable ceiling.
    EffectiveLimitExceeded,
    /// The selected profile disables the surface, so no advertised value executes.
    Disabled,
    /// The field is absent from the published record; absent is not unlimited.
    FieldNotAdvertised,
    /// The capability descriptor does not match trusted owner/profile/revision pins.
    DescriptorStale,
    /// The capability descriptor fails its own integrity digest.
    DescriptorTampered,
    /// The record was published for a different surface or owner revision.
    ProfileMismatch,
    /// No consumer pin is recorded for the repository.
    ConsumerNotRecorded,
    /// The recorded consumer pin has not adopted this effective-limit revision.
    ConsumerPinNotAdopted,
}

impl UnavailableReason {
    /// The stable machine-readable code for this reason.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::EffectiveLimitExceeded => "effective_limit_exceeded",
            Self::Disabled => "disabled",
            Self::FieldNotAdvertised => "field_not_advertised",
            Self::DescriptorStale => "descriptor_stale",
            Self::DescriptorTampered => "descriptor_tampered",
            Self::ProfileMismatch => "profile_mismatch",
            Self::ConsumerNotRecorded => "consumer_not_recorded",
            Self::ConsumerPinNotAdopted => "consumer_pin_not_adopted",
        }
    }
}

impl std::fmt::Display for UnavailableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for UnavailableReason {}

/// One classified value. `executable_ceiling` is authoritative; the two schema ceilings are
/// published only to make an intentional divergence visible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitRow {
    /// Field name in the matching capability descriptor's `effective_limits` object.
    pub field: String,
    pub class: LimitClass,
    /// Portable policy-schema maximum; `None` when no policy field exists for the value.
    pub policy_schema_ceiling: Option<u64>,
    /// Maximum the capability schema permits this descriptor to publish.
    pub capabilities_schema_ceiling: u64,
    /// Effective ceiling for the selected owner/profile.
    pub executable_ceiling: u64,
    /// Owner validator that enforces the executable ceiling.
    pub validator: String,
}

impl LimitRow {
    #[must_use]
    pub fn new(
        field: impl Into<String>,
        class: LimitClass,
        policy_schema_ceiling: Option<u64>,
        capabilities_schema_ceiling: u64,
        executable_ceiling: u64,
        validator: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            class,
            policy_schema_ceiling,
            capabilities_schema_ceiling,
            executable_ceiling,
            validator: validator.into(),
        }
    }

    /// A portable policy field that executes only up to the selected profile ceiling.
    #[must_use]
    pub fn policy(
        field: impl Into<String>,
        policy_schema_ceiling: u64,
        capabilities_schema_ceiling: u64,
        executable_ceiling: u64,
        validator: impl Into<String>,
    ) -> Self {
        let class = if policy_schema_ceiling == executable_ceiling {
            LimitClass::SchemaExecutableEqual
        } else {
            LimitClass::SchemaBroaderThanExecutable
        };
        Self::new(
            field,
            class,
            Some(policy_schema_ceiling),
            capabilities_schema_ceiling,
            executable_ceiling,
            validator,
        )
    }

    /// A parsed ceiling selected per corpus/profile; the portable policy schema has no such field.
    #[must_use]
    pub fn profile_selected(
        field: impl Into<String>,
        schema_ceiling: u64,
        executable_ceiling: u64,
        validator: impl Into<String>,
    ) -> Self {
        Self::new(
            field,
            LimitClass::ProfileSelected,
            None,
            schema_ceiling,
            executable_ceiling,
            validator,
        )
    }

    /// A runtime or transport guard with no portable policy field.
    #[must_use]
    pub fn runtime_guard(
        field: impl Into<String>,
        capabilities_schema_ceiling: u64,
        executable_ceiling: u64,
        validator: impl Into<String>,
    ) -> Self {
        Self::new(
            field,
            LimitClass::RuntimeGuard,
            None,
            capabilities_schema_ceiling,
            executable_ceiling,
            validator,
        )
    }

    fn validate(&self) -> Result<(), LimitRecordError> {
        if !valid_token(&self.field) || self.validator.trim().is_empty() {
            return Err(LimitRecordError::InvalidRecord);
        }
        if self.executable_ceiling == 0
            || self.capabilities_schema_ceiling < self.executable_ceiling
        {
            return Err(LimitRecordError::InvalidRecord);
        }
        match (self.class, self.policy_schema_ceiling) {
            (LimitClass::SchemaExecutableEqual, Some(policy))
                if policy == self.executable_ceiling
                    && self.capabilities_schema_ceiling == self.executable_ceiling => {}
            (LimitClass::SchemaBroaderThanExecutable, Some(policy))
                if policy > self.executable_ceiling
                    && policy >= self.capabilities_schema_ceiling => {}
            (LimitClass::ProfileSelected | LimitClass::RuntimeGuard, None) => {}
            _ => return Err(LimitRecordError::InvalidRecord),
        }
        Ok(())
    }
}

/// Owner-published classification of every advertised value for one surface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectiveLimitRecord {
    pub schema: String,
    pub surface: String,
    pub owner: String,
    pub owner_revision: String,
    pub capability_schema: String,
    pub capability_descriptor_sha256: String,
    /// The selected profile currently advertises at least one executable operation/value.
    pub enabled: bool,
    pub rows: Vec<LimitRow>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LimitRecordError {
    InvalidRecord,
    DuplicateField,
}

impl EffectiveLimitRecord {
    /// Validate the record before any value from it is trusted.
    ///
    /// # Errors
    ///
    /// Returns [`LimitRecordError`] for a malformed record, row, or duplicate field set.
    pub fn validate(&self) -> Result<(), LimitRecordError> {
        if self.schema != EFFECTIVE_LIMIT_RECORD_SCHEMA
            || !valid_token(&self.surface)
            || !valid_token(&self.owner)
            || !valid_token(&self.owner_revision)
            || !valid_token(&self.capability_schema)
            || !valid_sha256(&self.capability_descriptor_sha256)
            || self.rows.is_empty()
        {
            return Err(LimitRecordError::InvalidRecord);
        }
        let fields = self
            .rows
            .iter()
            .map(|row| row.field.as_str())
            .collect::<BTreeSet<_>>();
        if fields.len() != self.rows.len() {
            return Err(LimitRecordError::DuplicateField);
        }
        for row in &self.rows {
            row.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn row(&self, field: &str) -> Option<&LimitRow> {
        self.rows.iter().find(|row| row.field == field)
    }

    #[must_use]
    pub fn executable_ceiling(&self, field: &str) -> Option<u64> {
        self.row(field).map(|row| row.executable_ceiling)
    }

    /// Selected-profile admissibility for an already-validated record. Returns
    /// [`UnavailableReason`] when disabled, absent, or above the executable ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when the profile is disabled, the field is not advertised,
    /// or the requested value exceeds the executable ceiling.
    pub fn admit(&self, field: &str, requested: u64) -> Result<(), UnavailableReason> {
        if !self.enabled {
            return Err(UnavailableReason::Disabled);
        }
        let row = self
            .row(field)
            .ok_or(UnavailableReason::FieldNotAdvertised)?;
        if requested > row.executable_ceiling {
            return Err(UnavailableReason::EffectiveLimitExceeded);
        }
        Ok(())
    }

    /// Authenticate the complete record against the record derived from the trusted capability
    /// descriptor. `trusted` must never be derived from, or be a clone of, the record under test.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] on a malformed, mismatched, stale, or tampered record.
    pub fn authenticate(&self, trusted: &EffectiveLimitRecord) -> Result<(), UnavailableReason> {
        trusted
            .validate()
            .map_err(|_| UnavailableReason::DescriptorTampered)?;
        if self.surface != trusted.surface || self.owner_revision != trusted.owner_revision {
            return Err(UnavailableReason::ProfileMismatch);
        }
        if self.capability_descriptor_sha256 != trusted.capability_descriptor_sha256 {
            return Err(UnavailableReason::DescriptorStale);
        }
        if self != trusted {
            return Err(UnavailableReason::DescriptorTampered);
        }
        Ok(())
    }

    /// Admit a value only after authenticating the complete record against the trusted derivation.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] for an invalid, mismatched, stale, or oversized request.
    pub fn admit_authorized(
        &self,
        trusted: &EffectiveLimitRecord,
        field: &str,
        requested: u64,
    ) -> Result<(), UnavailableReason> {
        self.authenticate(trusted)?;
        trusted.admit(field, requested)
    }
}

/// Consumer adoption state for the producer's effective-limit revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Adoption {
    /// The consumer validates the current capability revision and discloses effective limits.
    Aligned,
    /// The consumer cannot yet discover executable ceilings, so every value stays unavailable.
    Pending,
}

/// One consumer surface recorded in the console-owned pin.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerSurface {
    pub surface: String,
    /// Capability schema the consumer copy validates, when it validates one at all.
    pub advertised_capability_schema: Option<String>,
    /// Whether the consumer can discover executable ceilings from its copy.
    pub effective_limits_advertised: bool,
}

/// The console-owned record of this repository's producer/consumer adoption.
///
/// The harness owns the authoritative pin matrix; this is the consumer's own assertion that it
/// reads the current capability revision and discloses effective limits. Digests of the copied
/// producer artifacts are pinned in [`contract_pins`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerPin {
    pub repository: String,
    pub producer_revision: String,
    pub adoption: Adoption,
    pub surfaces: Vec<ConsumerSurface>,
}

/// The console's default v3 consumer disclosure. The authoritative harness matrix is maintained
/// separately with the merged consumer/CI pins; this does not update it.
#[must_use]
pub fn console_consumer_pin() -> ConsumerPin {
    console_consumer_pin_for(crate::CapabilityVersion::V3)
}

/// A rollback advertisement cannot claim effective-limit adoption.
#[must_use]
pub fn console_consumer_pin_for(version: crate::CapabilityVersion) -> ConsumerPin {
    let aligned = version == crate::CapabilityVersion::V3;
    let suffix = if aligned { "v3" } else { "v1" };
    ConsumerPin {
        repository: CONSUMER_REPOSITORY.to_owned(),
        producer_revision: PRODUCER_REVISION.to_owned(),
        adoption: if aligned {
            Adoption::Aligned
        } else {
            Adoption::Pending
        },
        surfaces: vec![
            ConsumerSurface {
                surface: "context-memory".to_owned(),
                advertised_capability_schema: Some(format!(
                    "ascension.context-memory.capabilities.{suffix}"
                )),
                effective_limits_advertised: aligned,
            },
            ConsumerSurface {
                surface: "provider-session".to_owned(),
                advertised_capability_schema: Some(format!(
                    "ascension.provider-session.capabilities.{suffix}"
                )),
                effective_limits_advertised: aligned,
            },
        ],
    }
}

/// Consumer-side admission: a value may be presented only when the pinned consumer can discover
/// the effective limit and the authenticated record admits it. An absent surface is never
/// unlimited.
///
/// # Errors
///
/// Returns [`UnavailableReason`] for an unrecorded consumer, a pending adoption, an absent or
/// undisclosed surface, a mismatched schema, or a value above the executable ceiling.
pub fn admit_consumer(
    pin: &ConsumerPin,
    surface: &str,
    record: &EffectiveLimitRecord,
    trusted: &EffectiveLimitRecord,
    field: &str,
    requested: u64,
) -> Result<(), UnavailableReason> {
    if pin.repository != CONSUMER_REPOSITORY {
        return Err(UnavailableReason::ConsumerNotRecorded);
    }
    if pin.adoption == Adoption::Pending {
        return Err(UnavailableReason::ConsumerPinNotAdopted);
    }
    let entry = pin
        .surfaces
        .iter()
        .find(|entry| entry.surface == surface)
        .ok_or(UnavailableReason::FieldNotAdvertised)?;
    let schema_matches =
        entry.advertised_capability_schema.as_deref() == Some(record.capability_schema.as_str());
    if !entry.effective_limits_advertised || !schema_matches {
        return Err(UnavailableReason::FieldNotAdvertised);
    }
    record.authenticate(trusted)?;
    trusted.admit(field, requested)
}

/// Mirrors the producer schema token pattern `^[A-Za-z0-9][A-Za-z0-9._:-]*$`, which admits a
/// colon but requires the first byte to be alphanumeric.
fn valid_token(value: &str) -> bool {
    let mut bytes = value.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    value.len() <= 128 && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Copied producer artifact digests and the four producer schemas, pinned by exact bytes.
///
/// The digests are verified against the copied `contracts/` bytes by the module tests. All claims
/// are synthetic/contract-level; the bytes are the harness producer's, copied without edit.
pub mod contract_pins {
    /// SHA-256 of the copied context-memory policy schema.
    pub const MEMORY_POLICY_SCHEMA_SHA256: &str =
        "55c9bba8ec71ae00b1a6c6bec07b1ca85ec9df0de0d7a0bfa2b5cd663f9aa416";
    /// SHA-256 of the copied context-memory capabilities schema (the descriptor digest).
    pub const MEMORY_CAPABILITIES_SCHEMA_SHA256: &str =
        "2b980bbdcdd886398c1e590303b82afee174e4569164e79b210ab35f6669bd22";
    /// SHA-256 of the copied provider-session policy schema.
    pub const SESSION_POLICY_SCHEMA_SHA256: &str =
        "48d6dc1c75504983c3d5e6a1152c0447874eeb512e45b276ab440962e52781a5";
    /// SHA-256 of the copied provider-session capabilities schema (the descriptor digest).
    pub const SESSION_CAPABILITIES_SCHEMA_SHA256: &str =
        "de1348ec7434703722b00a2ddb4bcef7cec02b3788ff14af018cf7b7efaf21f0";
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    const DESCRIPTOR: &str = "2b980bbdcdd886398c1e590303b82afee174e4569164e79b210ab35f6669bd22";

    fn digest(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn trusted() -> EffectiveLimitRecord {
        EffectiveLimitRecord {
            schema: EFFECTIVE_LIMIT_RECORD_SCHEMA.to_owned(),
            surface: "context-memory".to_owned(),
            owner: "sts2-harness".to_owned(),
            owner_revision: "harness-context-memory-v3".to_owned(),
            capability_schema: "ascension.context-memory.capabilities.v3".to_owned(),
            capability_descriptor_sha256: DESCRIPTOR.to_owned(),
            enabled: true,
            rows: vec![
                LimitRow::policy("max_candidates", 64, 64, 64, "MemoryPolicy"),
                LimitRow::runtime_guard("max_source_bytes", 65536, 65536, "MemoryCorpus"),
            ],
        }
    }

    #[test]
    fn copied_capability_artifacts_match_pinned_producer_digests() {
        let memory_caps =
            include_bytes!("../../../contracts/context-memory/capabilities.schema.json");
        let session_caps =
            include_bytes!("../../../contracts/provider-session/capabilities.schema.json");
        let memory_policy = include_bytes!("../../../contracts/context-memory/policy.schema.json");
        let session_policy =
            include_bytes!("../../../contracts/provider-session/policy.schema.json");
        assert_eq!(
            digest(memory_caps),
            contract_pins::MEMORY_CAPABILITIES_SCHEMA_SHA256
        );
        assert_eq!(
            digest(session_caps),
            contract_pins::SESSION_CAPABILITIES_SCHEMA_SHA256
        );
        assert_eq!(
            digest(memory_policy),
            contract_pins::MEMORY_POLICY_SCHEMA_SHA256
        );
        assert_eq!(
            digest(session_policy),
            contract_pins::SESSION_POLICY_SCHEMA_SHA256
        );
    }

    #[test]
    fn unavailable_reasons_are_machine_readable() {
        let codes = [
            (
                UnavailableReason::EffectiveLimitExceeded,
                "effective_limit_exceeded",
            ),
            (UnavailableReason::Disabled, "disabled"),
            (
                UnavailableReason::FieldNotAdvertised,
                "field_not_advertised",
            ),
            (UnavailableReason::DescriptorStale, "descriptor_stale"),
            (UnavailableReason::DescriptorTampered, "descriptor_tampered"),
            (UnavailableReason::ProfileMismatch, "profile_mismatch"),
            (
                UnavailableReason::ConsumerNotRecorded,
                "consumer_not_recorded",
            ),
            (
                UnavailableReason::ConsumerPinNotAdopted,
                "consumer_pin_not_adopted",
            ),
        ];
        for (reason, code) in codes {
            assert_eq!(reason.code(), code);
            let encoded = serde_json::to_value(reason).expect("reason encodes");
            assert_eq!(encoded.as_str(), Some(code));
        }
    }

    #[test]
    fn value_above_executable_ceiling_is_never_presented() {
        let trusted = trusted();
        assert_eq!(
            trusted.admit_authorized(&trusted, "max_candidates", 64),
            Ok(())
        );
        assert_eq!(
            trusted.admit_authorized(&trusted, "max_candidates", 65),
            Err(UnavailableReason::EffectiveLimitExceeded)
        );
        assert_eq!(
            trusted.admit_authorized(&trusted, "absent_field", 1),
            Err(UnavailableReason::FieldNotAdvertised)
        );
    }

    #[test]
    fn disabled_record_rejects_every_field() {
        let mut disabled = trusted();
        disabled.enabled = false;
        assert_eq!(
            disabled.admit_authorized(&disabled, "max_candidates", 1),
            Err(UnavailableReason::Disabled)
        );
    }

    #[test]
    fn tampered_record_with_trusted_label_fails_closed() {
        let trusted = trusted();

        let mut owner_changed = trusted.clone();
        owner_changed.owner = "other-owner".to_owned();
        assert_eq!(
            owner_changed.authenticate(&trusted),
            Err(UnavailableReason::DescriptorTampered)
        );

        let mut enabled_changed = trusted.clone();
        enabled_changed.enabled = false;
        assert_eq!(
            enabled_changed.authenticate(&trusted),
            Err(UnavailableReason::DescriptorTampered)
        );

        let mut ceiling_changed = trusted.clone();
        ceiling_changed.rows[0].executable_ceiling = 128;
        assert_eq!(
            ceiling_changed.authenticate(&trusted),
            Err(UnavailableReason::DescriptorTampered)
        );

        let mut class_changed = trusted.clone();
        class_changed.rows[0].class = LimitClass::RuntimeGuard;
        assert_eq!(
            class_changed.authenticate(&trusted),
            Err(UnavailableReason::DescriptorTampered)
        );

        let mut schema_ceiling_changed = trusted.clone();
        schema_ceiling_changed.rows[0].capabilities_schema_ceiling = 128;
        assert_eq!(
            schema_ceiling_changed.authenticate(&trusted),
            Err(UnavailableReason::DescriptorTampered)
        );

        let mut policy_ceiling_changed = trusted.clone();
        policy_ceiling_changed.rows[0].policy_schema_ceiling = Some(128);
        assert_eq!(
            policy_ceiling_changed.authenticate(&trusted),
            Err(UnavailableReason::DescriptorTampered)
        );
    }

    #[test]
    fn mismatched_or_stale_record_rejects_before_admission() {
        let trusted = trusted();

        let mut other_surface = trusted.clone();
        other_surface.surface = "provider-session".to_owned();
        assert_eq!(
            other_surface.authenticate(&trusted),
            Err(UnavailableReason::ProfileMismatch)
        );

        let mut other_revision = trusted.clone();
        other_revision.owner_revision = "harness-provider-session-v3".to_owned();
        assert_eq!(
            other_revision.authenticate(&trusted),
            Err(UnavailableReason::ProfileMismatch)
        );

        let mut other_descriptor = trusted.clone();
        other_descriptor.capability_descriptor_sha256 = "0".repeat(64);
        assert_eq!(
            other_descriptor.authenticate(&trusted),
            Err(UnavailableReason::DescriptorStale)
        );
    }

    #[test]
    fn consumer_pin_gates_admission() {
        let trusted = trusted();
        let mut pin = console_consumer_pin();
        // Synthetic future adoption, not the current served configuration.
        pin.adoption = Adoption::Aligned;
        for surface in &mut pin.surfaces {
            surface.advertised_capability_schema =
                Some(format!("ascension.{}.capabilities.v3", surface.surface));
            surface.effective_limits_advertised = true;
        }
        assert_eq!(
            admit_consumer(
                &pin,
                "context-memory",
                &trusted,
                &trusted,
                "max_candidates",
                64
            ),
            Ok(())
        );
        assert_eq!(
            admit_consumer(
                &pin,
                "context-memory",
                &trusted,
                &trusted,
                "max_candidates",
                65
            ),
            Err(UnavailableReason::EffectiveLimitExceeded)
        );

        let mut pending = pin.clone();
        pending.adoption = Adoption::Pending;
        assert_eq!(
            admit_consumer(
                &pending,
                "context-memory",
                &trusted,
                &trusted,
                "max_candidates",
                1
            ),
            Err(UnavailableReason::ConsumerPinNotAdopted)
        );

        assert_eq!(
            admit_consumer(
                &pin,
                "provider-session",
                &trusted,
                &trusted,
                "max_candidates",
                1
            ),
            Err(UnavailableReason::FieldNotAdvertised)
        );

        let mut foreign = pin.clone();
        foreign.repository = "AI-Ascension/other".to_owned();
        assert_eq!(
            admit_consumer(
                &foreign,
                "context-memory",
                &trusted,
                &trusted,
                "max_candidates",
                1
            ),
            Err(UnavailableReason::ConsumerNotRecorded)
        );
    }

    #[test]
    fn record_tokens_follow_the_producer_pattern() {
        // Producer pattern `^[A-Za-z0-9][A-Za-z0-9._:-]*$` admits a colon after the first byte.
        assert!(valid_token("thread:read"));
        assert!(valid_token("ns:sub:field"));
        assert!(valid_token("a1._:-z"));
        assert!(!valid_token(":leading"));
        assert!(!valid_token("-leading"));
        assert!(!valid_token(""));
        assert!(!valid_token("has space"));
        assert!(!valid_token("has/slash"));

        let mut colon_field = trusted();
        colon_field.rows[0].field = "ns:max_candidates".to_owned();
        assert_eq!(colon_field.validate(), Ok(()));
    }

    // NOTE (real-record conformance unverified): this exercises the local fixture derivation only.
    // The producer-record equality check must stay fail-closed until a real harness record is
    // compared, because the hardcoded class/validator/ceiling mapping is not yet proven.
    #[test]
    fn admission_uses_the_descriptor_derived_record_as_the_trusted_side() {
        let route_record = trusted();
        // A clone of the derivation admits, because the trusted side is the derived record.
        let adopted = route_record.clone();
        assert_eq!(
            adopted.admit_authorized(&route_record, "max_candidates", 64),
            Ok(())
        );

        // A record-under-test that changes a ceiling while keeping the trusted label fails against
        // the derivation even when the descriptor digest still looks trusted.
        let mut ceiling_changed = route_record.clone();
        ceiling_changed.rows[0].executable_ceiling = 128;
        assert_eq!(
            ceiling_changed.admit_authorized(&route_record, "max_candidates", 64),
            Err(UnavailableReason::DescriptorTampered)
        );

        // A valid record-under-test with the trusted label but a different owner also fails
        // against the derivation.
        let mut owner_changed = route_record.clone();
        owner_changed.owner = "other-owner".to_owned();
        assert_eq!(
            owner_changed.admit_authorized(&route_record, "max_candidates", 64),
            Err(UnavailableReason::DescriptorTampered)
        );

        // If the record under test were (wrongly) supplied as its own trusted side it would pass,
        // showing the gate's guarantee depends on the independent descriptor derivation.
        assert_eq!(owner_changed.authenticate(&owner_changed), Ok(()));
    }
}
