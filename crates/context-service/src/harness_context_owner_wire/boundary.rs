use serde::{Deserialize, Serialize};

use super::validation::{OwnerWireError, validate_digest, validate_identifier, validate_schema};

pub const CONTEXT_OWNER_BINDING_SCHEMA_V1: &str = "ascension.context-control.owner-binding.v1";
pub const CONTEXT_OWNER_ASSOCIATION_VIEW_SCHEMA_V1: &str =
    "ascension.harness.context-owner-association-view.v1";
pub const CONTEXT_OWNER_EFFECTIVE_LIMITS_VIEW_SCHEMA_V1: &str =
    "ascension.harness.context-owner-effective-limits-view.v1";

/// Exact Harness context boundary shape. Keep field names, order, and integer representation
/// aligned with `ContextBoundary`; it is included in mutation, publication, adoption, and control
/// records and is not reducible to an epoch tuple.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBoundary {
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
    pub state_id: String,
    pub generation: u64,
    pub observation_sha256: String,
    pub catalog_sha256: String,
    pub adapter_revision: String,
    pub model_revision: String,
    pub configuration_sha256: String,
    pub output_schema_sha256: String,
    pub controller_epoch: u64,
    pub gate_epoch: u64,
    pub control_version: u64,
}

impl ContextBoundary {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        for (field, value) in [
            ("boundary_run_id", self.run_id.as_str()),
            ("boundary_episode_id", self.episode_id.as_str()),
            ("boundary_agent_id", self.agent_id.as_str()),
            ("boundary_state_id", self.state_id.as_str()),
            ("boundary_adapter_revision", self.adapter_revision.as_str()),
            ("boundary_model_revision", self.model_revision.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        for (field, value) in [
            (
                "boundary_observation_sha256",
                self.observation_sha256.as_str(),
            ),
            ("boundary_catalog_sha256", self.catalog_sha256.as_str()),
            (
                "boundary_configuration_sha256",
                self.configuration_sha256.as_str(),
            ),
            (
                "boundary_output_schema_sha256",
                self.output_schema_sha256.as_str(),
            ),
        ] {
            validate_digest(field, value)?;
        }
        if self.generation == 0
            || self.controller_epoch == 0
            || self.gate_epoch == 0
            || self.control_version == 0
        {
            return Err(OwnerWireError::InvalidValue("boundary_epoch"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBindingState {
    Available,
    Disabled,
    Unattached,
    Denied,
    Stale,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBindingOperation {
    IncludeItem,
    ExcludeItem,
    PinItem,
    UnpinItem,
    PutNote,
    RemoveNote,
    SetObjective,
    RestoreConfiguration,
    Pause,
    Commit,
    Resume,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBindingSource {
    pub source_id: String,
    pub version: u64,
    pub digest: String,
}

impl ContextBindingSource {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_identifier("binding_source_id", &self.source_id)?;
        validate_digest("binding_source_digest", &self.digest)?;
        if self.version == 0 {
            return Err(OwnerWireError::InvalidValue("binding_source_version"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEffectiveLimits {
    pub max_items: u64,
    pub max_notes: u64,
    pub max_context_bytes: u64,
    pub max_objective_bytes: u64,
    pub max_control_events: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_reserve_bytes: Option<u64>,
}

impl ContextEffectiveLimits {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        if self.max_items == 0
            || self.max_items > super::validation::MAX_HARNESS_ITEMS as u64
            || self.max_notes > super::validation::MAX_HARNESS_NOTES as u64
            || self.max_context_bytes == 0
            || self.max_context_bytes > super::validation::MAX_HARNESS_CONTEXT_BYTES as u64
            || self.max_objective_bytes == 0
            || self.max_objective_bytes > super::validation::MAX_HARNESS_OBJECTIVE_BYTES as u64
            || self.max_control_events == 0
            || self.output_reserve_bytes == Some(0)
        {
            return Err(OwnerWireError::OutOfBounds("effective_limits"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBindingContinuity {
    pub survives_controller_restart: bool,
    pub receipt_recovery: bool,
    pub provider_session_continuity: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBindingGrants {
    pub metadata_read: bool,
    pub content_read: bool,
    pub edit: bool,
    pub control: bool,
}

impl ContextBindingGrants {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        if (self.content_read && !self.metadata_read)
            || (self.edit && !self.content_read)
            || (self.control && !self.metadata_read)
        {
            return Err(OwnerWireError::InvalidValue("binding_grants"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerBinding {
    pub schema_version: String,
    pub owner_id: String,
    pub owner_version: String,
    pub invocation_id: String,
    pub binding_id: String,
    pub binding_version: u64,
    pub binding_digest: String,
    pub context_ref: String,
    pub instance_id: String,
    pub node_kind: String,
    pub state: ContextBindingState,
    pub workflow_run_id: String,
    pub definition_digest: String,
    pub graph_id: String,
    pub node_id: String,
    pub node_execution_id: String,
    pub boundary: ContextBoundary,
    pub lease_epoch: u64,
    pub snapshot_id: String,
    pub approved_revision_id: String,
    pub plan_epoch: u64,
    pub grants: ContextBindingGrants,
    pub continuity: ContextBindingContinuity,
}

impl ContextOwnerBinding {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema_version, CONTEXT_OWNER_BINDING_SCHEMA_V1)?;
        for (field, value) in [
            ("binding_owner_id", self.owner_id.as_str()),
            ("binding_owner_version", self.owner_version.as_str()),
            ("binding_invocation_id", self.invocation_id.as_str()),
            ("binding_id", self.binding_id.as_str()),
            ("binding_context_ref", self.context_ref.as_str()),
            ("binding_instance_id", self.instance_id.as_str()),
            ("binding_node_kind", self.node_kind.as_str()),
            ("binding_run_id", self.workflow_run_id.as_str()),
            ("binding_graph_id", self.graph_id.as_str()),
            ("binding_node_id", self.node_id.as_str()),
            ("binding_node_execution_id", self.node_execution_id.as_str()),
            ("binding_snapshot_id", self.snapshot_id.as_str()),
            (
                "binding_approved_revision_id",
                self.approved_revision_id.as_str(),
            ),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("binding_digest", &self.binding_digest)?;
        validate_digest("binding_definition_digest", &self.definition_digest)?;
        self.boundary.validate()?;
        self.grants.validate()?;
        if self.binding_version == 0
            || self.lease_epoch == 0
            || self.plan_epoch == 0
            || self.workflow_run_id != self.boundary.run_id
            || (self.state == ContextBindingState::Available && !self.grants.metadata_read)
        {
            return Err(OwnerWireError::InvalidValue("owner_binding"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerAssociationView {
    pub schema_version: String,
    pub binding: ContextOwnerBinding,
}

impl ContextOwnerAssociationView {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_ASSOCIATION_VIEW_SCHEMA_V1,
        )?;
        self.binding.validate()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerEffectiveLimitsView {
    pub schema_version: String,
    pub owner_id: String,
    pub owner_version: String,
    pub catalog_digest: String,
    pub binding_id: String,
    pub binding_version: u64,
    pub binding_digest: String,
    pub context_ref: String,
    pub node_kind: String,
    pub adapter_revision: String,
    pub model_revision: String,
    pub effective_limits: ContextEffectiveLimits,
}

impl ContextOwnerEffectiveLimitsView {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_EFFECTIVE_LIMITS_VIEW_SCHEMA_V1,
        )?;
        for (field, value) in [
            ("limits_owner_id", self.owner_id.as_str()),
            ("limits_owner_version", self.owner_version.as_str()),
            ("limits_binding_id", self.binding_id.as_str()),
            ("limits_context_ref", self.context_ref.as_str()),
            ("limits_node_kind", self.node_kind.as_str()),
            ("limits_adapter_revision", self.adapter_revision.as_str()),
            ("limits_model_revision", self.model_revision.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("limits_catalog_digest", &self.catalog_digest)?;
        validate_digest("limits_binding_digest", &self.binding_digest)?;
        self.effective_limits.validate()?;
        if self.binding_version == 0 {
            return Err(OwnerWireError::InvalidValue("limits_binding_version"));
        }
        Ok(())
    }
}
