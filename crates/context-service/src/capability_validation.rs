// SPDX-License-Identifier: MIT

//! Validation of the copied v3 descriptor contracts. Integrity is not owner authentication:
//! attached routes additionally compare with an independently configured, scoped descriptor.

use crate::effective_limits::{UnavailableReason, contract_pins};
use crate::{MemoryCapabilities, SessionCapabilitiesView};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Explicit advertisement choice. V1 rollback omits executable-limit disclosure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CapabilityVersion {
    V1,
    #[default]
    V3,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn token(value: &str) -> bool {
    let mut bytes = value.bytes();
    !value.is_empty()
        && value.len() <= 128
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn status(value: &str) -> bool {
    matches!(value, "supported" | "unsupported" | "unverified")
}

impl MemoryCapabilities {
    /// Digest the exact typed producer serialization with the self-digest cleared.
    pub fn descriptor_digest(&self) -> Result<String, UnavailableReason> {
        let mut unsigned = self.clone();
        unsigned.binding.descriptor_sha256.clear();
        serde_json::to_vec(&unsigned)
            .map(|bytes| digest(&bytes))
            .map_err(|_| UnavailableReason::DescriptorTampered)
    }

    /// Validate bounded shape, pinned identities and payload integrity, not remote authority.
    pub fn validate_descriptor(&self) -> Result<(), UnavailableReason> {
        let binding = &self.binding;
        if self.schema != crate::MEMORY_CAPABILITIES_SCHEMA
            || self.product_phase != 3
            || ![
                &self.scope.project_id,
                &self.scope.run_id,
                &self.scope.episode_id,
                &self.scope.agent_id,
            ]
            .into_iter()
            .all(|value| token(value))
            || ![
                &self.local_lexical_retrieval,
                &self.extractive_compaction,
                &self.abstractive_adapter,
                &self.per_decision_policy,
            ]
            .into_iter()
            .all(|value| status(value))
            || !self.phase2_approval_required
            || self.persistent_provider_sessions
            || self.provider_side_compaction
            || self.semantic_vector_retrieval != "unsupported"
            || self.hidden_reasoning_access
            || self.direct_game_dispatch
            || self.effective_limits.policy_schema != "ascension.context-memory.policy.v1"
            || binding.owner != "sts2-harness"
            || binding.owner_revision != "harness-context-memory-v3"
            || binding.model_revision != "not-applicable"
            || binding.adapter_revision != "harness-context-memory-v3"
            || binding.adapter_revision_sha256 != digest(binding.adapter_revision.as_bytes())
            || binding.policy_schema_sha256 != contract_pins::MEMORY_POLICY_SCHEMA_SHA256
            || !hash(&binding.descriptor_sha256)
            || self.supported_operations.len() > 9
            || self
                .supported_operations
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.supported_operations.len()
            || self.supported_operations.iter().any(|operation| {
                !matches!(
                    operation.as_str(),
                    "search"
                        | "extract"
                        | "generate"
                        | "review"
                        | "select"
                        | "policy"
                        | "adopt"
                        | "revoke"
                        | "evaluate"
                )
            })
            || (!self.enabled && !self.supported_operations.is_empty())
        {
            return Err(UnavailableReason::DescriptorTampered);
        }
        self.effective_limit_record()
            .validate()
            .map_err(|_| UnavailableReason::DescriptorTampered)?;
        if binding.descriptor_sha256 != self.descriptor_digest()? {
            return Err(UnavailableReason::DescriptorTampered);
        }
        Ok(())
    }
}

impl SessionCapabilitiesView {
    /// Digest the exact typed producer serialization with the self-digest cleared.
    pub fn descriptor_digest(&self) -> Result<String, UnavailableReason> {
        let mut unsigned = self.clone();
        unsigned.binding.descriptor_sha256.clear();
        serde_json::to_vec(&unsigned)
            .map(|bytes| digest(&bytes))
            .map_err(|_| UnavailableReason::DescriptorTampered)
    }

    /// Validate bounded shape, pinned policy and payload integrity, not native/profile authority.
    pub fn validate_descriptor(&self) -> Result<(), UnavailableReason> {
        let binding = &self.binding;
        if self.schema != crate::SESSION_CAPABILITIES_SCHEMA
            || !token(&self.profile_id)
            || !token(&self.native_version)
            || ![
                &self.profile_sha256,
                &self.native_binary_sha256,
                &self.native_schema_sha256,
                &binding.descriptor_sha256,
            ]
            .into_iter()
            .all(|value| hash(value))
            || !matches!(
                self.evidence.as_str(),
                "schema_only" | "compiled_peer" | "native_binary_fake_upstream" | "live_provider"
            )
            || self.transport != "owned_stdio"
            || self.enabled_methods.is_empty()
            || self.enabled_methods.len() > 32
            || self.enabled_methods.iter().collect::<BTreeSet<_>>().len()
                != self.enabled_methods.len()
            || self.enabled_methods.iter().any(|method| {
                method.is_empty()
                    || method.len() > 128
                    || !method
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
            })
            || self.hardening.tools_enabled
            || self.hardening.ambient_history
            || !matches!(
                self.hardening.transform_handling.as_str(),
                "verified_suppressed" | "detect_and_fence" | "opaque_approved"
            )
            || self.unknown_methods != "deny"
            || self.raw_rpc
            || self.effective_limits.policy_schema != "ascension.provider-session.policy.v1"
            || binding.owner != "sts2-harness"
            || binding.owner_revision != "harness-provider-session-v3"
            || binding.model_revision != self.native_version
            || binding.adapter_revision != self.profile_id
            || binding.adapter_revision_sha256 != self.profile_sha256
            || binding.policy_schema_sha256 != contract_pins::SESSION_POLICY_SCHEMA_SHA256
        {
            return Err(UnavailableReason::DescriptorTampered);
        }
        self.effective_limit_record()
            .validate()
            .map_err(|_| UnavailableReason::DescriptorTampered)?;
        if binding.descriptor_sha256 != self.descriptor_digest()? {
            return Err(UnavailableReason::DescriptorTampered);
        }
        Ok(())
    }
}
