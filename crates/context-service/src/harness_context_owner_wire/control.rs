use serde::{Deserialize, Serialize};

use super::boundary::{ContextBoundary, ContextOwnerBinding};
use super::validation::{OwnerWireError, validate_digest, validate_identifier, validate_schema};

pub const CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2: &str =
    "ascension.context-control.owner-receipt.v2";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextControlCommandKind {
    Pause,
    Commit,
    Resume,
}

/// Exact external-tagged Harness command. The command itself, including its full boundary and
/// both commit manifest digests, is the control-receipt recovery key.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextControlCommand {
    Pause {
        idempotency_key: String,
        expected_control_version: u64,
    },
    Commit {
        idempotency_key: String,
        expected_control_version: u64,
        expected_revision_id: String,
        expected_boundary: ContextBoundary,
        preview_manifest_digest: String,
        approved_manifest_digest: String,
    },
    Resume {
        idempotency_key: String,
        expected_control_version: u64,
        expected_boundary: ContextBoundary,
    },
}

impl ContextControlCommand {
    #[must_use]
    pub fn kind(&self) -> ContextControlCommandKind {
        match self {
            Self::Pause { .. } => ContextControlCommandKind::Pause,
            Self::Commit { .. } => ContextControlCommandKind::Commit,
            Self::Resume { .. } => ContextControlCommandKind::Resume,
        }
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        match self {
            Self::Pause {
                idempotency_key, ..
            }
            | Self::Commit {
                idempotency_key, ..
            }
            | Self::Resume {
                idempotency_key, ..
            } => idempotency_key,
        }
    }

    pub fn validate(&self, binding: &ContextOwnerBinding) -> Result<(), OwnerWireError> {
        binding.validate()?;
        validate_identifier("control_idempotency_key", self.idempotency_key())?;
        match self {
            Self::Pause {
                expected_control_version,
                ..
            } => {
                if *expected_control_version != binding.boundary.control_version {
                    return Err(OwnerWireError::CorrelationMismatch(
                        "control_expected_version",
                    ));
                }
            }
            Self::Commit {
                expected_control_version,
                expected_revision_id,
                expected_boundary,
                preview_manifest_digest,
                approved_manifest_digest,
                ..
            } => {
                validate_identifier("control_expected_revision_id", expected_revision_id)?;
                validate_digest("control_preview_manifest", preview_manifest_digest)?;
                validate_digest("control_approved_manifest", approved_manifest_digest)?;
                expected_boundary.validate()?;
                if *expected_control_version != binding.boundary.control_version
                    || expected_boundary != &binding.boundary
                    || preview_manifest_digest != approved_manifest_digest
                {
                    return Err(OwnerWireError::CorrelationMismatch("control_commit"));
                }
            }
            Self::Resume {
                expected_control_version,
                expected_boundary,
                ..
            } => {
                expected_boundary.validate()?;
                if *expected_control_version != binding.boundary.control_version
                    || expected_boundary != &binding.boundary
                {
                    return Err(OwnerWireError::CorrelationMismatch("control_resume"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextControlReceipt {
    pub schema_version: String,
    pub owner_id: String,
    pub invocation_id: String,
    pub binding_id: String,
    pub binding_digest: String,
    pub command: ContextControlCommandKind,
    pub command_id: String,
    pub idempotency_key: String,
    pub effect: String,
    pub control_version: u64,
    pub plan_epoch: u64,
    pub controller_epoch: u64,
    pub gate_epoch: u64,
    pub boundary: ContextBoundary,
    pub revision_id: Option<String>,
    pub preview_manifest_digest: Option<String>,
    pub approved_manifest_digest: Option<String>,
}

#[path = "control/adoption.rs"]
mod adoption;

impl ContextControlReceipt {
    pub fn validate_for(
        &self,
        binding: &ContextOwnerBinding,
        command: &ContextControlCommand,
    ) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2,
        )?;
        binding.validate()?;
        command.validate(binding)?;
        for (field, value) in [
            ("control_receipt_owner_id", self.owner_id.as_str()),
            ("control_receipt_invocation_id", self.invocation_id.as_str()),
            ("control_receipt_binding_id", self.binding_id.as_str()),
            ("control_receipt_command_id", self.command_id.as_str()),
            (
                "control_receipt_idempotency_key",
                self.idempotency_key.as_str(),
            ),
            ("control_receipt_effect", self.effect.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("control_receipt_binding_digest", &self.binding_digest)?;
        self.boundary.validate()?;
        let expected_boundary = resulting_boundary(binding, command)?;
        let expected_plan_epoch = match command {
            ContextControlCommand::Commit { .. } => binding
                .plan_epoch
                .checked_add(1)
                .ok_or(OwnerWireError::OutOfBounds("plan_epoch"))?,
            ContextControlCommand::Pause { .. } | ContextControlCommand::Resume { .. } => {
                binding.plan_epoch
            }
        };
        let expected_effect = match command {
            ContextControlCommand::Pause { .. } => "pause_requested",
            ContextControlCommand::Commit { .. } => "revision_committed",
            ContextControlCommand::Resume { .. } => "resume_accepted",
        };
        if self.owner_id != binding.owner_id
            || self.invocation_id != binding.invocation_id
            || self.binding_id != binding.binding_id
            || self.binding_digest != binding.binding_digest
            || self.command != command.kind()
            || self.idempotency_key != command.idempotency_key()
            || self.effect != expected_effect
            || self.boundary != expected_boundary
            || self.controller_epoch != expected_boundary.controller_epoch
            || self.gate_epoch != expected_boundary.gate_epoch
            || self.control_version != expected_boundary.control_version
            || self.plan_epoch != expected_plan_epoch
        {
            return Err(OwnerWireError::CorrelationMismatch("control_receipt"));
        }
        match command {
            ContextControlCommand::Commit {
                expected_revision_id,
                preview_manifest_digest,
                approved_manifest_digest,
                ..
            } => {
                let revision = self
                    .revision_id
                    .as_deref()
                    .ok_or(OwnerWireError::CorrelationMismatch("commit_revision"))?;
                validate_identifier("control_result_revision_id", revision)?;
                if revision == expected_revision_id
                    || self.preview_manifest_digest.as_deref()
                        != Some(preview_manifest_digest.as_str())
                    || self.approved_manifest_digest.as_deref()
                        != Some(approved_manifest_digest.as_str())
                {
                    return Err(OwnerWireError::CorrelationMismatch("commit_result"));
                }
            }
            ContextControlCommand::Pause { .. } | ContextControlCommand::Resume { .. } => {
                if self.revision_id.is_some()
                    || self.preview_manifest_digest.is_some()
                    || self.approved_manifest_digest.is_some()
                {
                    return Err(OwnerWireError::CorrelationMismatch("non_commit_fields"));
                }
            }
        }
        Ok(())
    }
}

fn resulting_boundary(
    binding: &ContextOwnerBinding,
    command: &ContextControlCommand,
) -> Result<ContextBoundary, OwnerWireError> {
    let mut result = match command {
        ContextControlCommand::Commit {
            expected_boundary, ..
        }
        | ContextControlCommand::Resume {
            expected_boundary, ..
        } => expected_boundary.clone(),
        ContextControlCommand::Pause { .. } => binding.boundary.clone(),
    };
    result.control_version = result
        .control_version
        .checked_add(1)
        .ok_or(OwnerWireError::OutOfBounds("control_version"))?;
    if !matches!(command, ContextControlCommand::Commit { .. }) {
        result.gate_epoch = result
            .gate_epoch
            .checked_add(1)
            .ok_or(OwnerWireError::OutOfBounds("gate_epoch"))?;
    }
    Ok(result)
}
