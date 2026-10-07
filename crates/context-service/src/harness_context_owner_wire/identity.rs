use serde::{Deserialize, Serialize};

use super::validation::{OwnerWireError, validate_correlation_text, validate_identifier};

pub const CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2: &str =
    "ascension.console.context-owner-invocation.v2";

/// Verified Console identity retained for request correlation. This is data, not a credential or
/// authority proof; constructors and ingress admission remain owned by the trusted server.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleOwnerIdentityV2 {
    pub issuer: String,
    pub subject: String,
    pub audience: String,
    pub credential_id: String,
    pub grant_id: String,
    pub grant_generation: u64,
    pub grant_expires_at: u64,
}

impl ConsoleOwnerIdentityV2 {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        for (field, value) in [
            ("console_issuer", self.issuer.as_str()),
            ("console_audience", self.audience.as_str()),
            ("console_credential_id", self.credential_id.as_str()),
        ] {
            validate_correlation_text(field, value)?;
        }
        for (field, value) in [
            ("console_subject", self.subject.as_str()),
            ("console_grant_id", self.grant_id.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        if self.grant_expires_at == 0 {
            return Err(OwnerWireError::InvalidValue("console_grant_lifetime"));
        }
        Ok(())
    }
}

/// Full Console request scope. It stays separate from Harness's workflow-run and binding IDs.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsoleOwnerScopeV2 {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

impl ConsoleOwnerScopeV2 {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        for (field, value) in [
            ("console_project_id", self.project_id.as_str()),
            ("console_run_id", self.run_id.as_str()),
            ("console_episode_id", self.episode_id.as_str()),
            ("console_agent_id", self.agent_id.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        Ok(())
    }
}

/// Independently resolved Harness identity. The reference identifier is non-secret metadata;
/// bearer bytes never belong in this wire record.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessActorIdentityV2 {
    pub actor_subject: String,
    pub owner_id: String,
    pub workflow_run_id: String,
    pub credential_reference_id: String,
    pub credential_expires_at: u64,
}

impl HarnessActorIdentityV2 {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        for (field, value) in [
            ("harness_actor_subject", self.actor_subject.as_str()),
            ("harness_owner_id", self.owner_id.as_str()),
            ("harness_workflow_run_id", self.workflow_run_id.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        validate_correlation_text(
            "harness_credential_reference_id",
            &self.credential_reference_id,
        )?;
        if self.credential_expires_at == 0 {
            return Err(OwnerWireError::InvalidValue("harness_credential_expiry"));
        }
        Ok(())
    }
}

/// Local correlation envelope. Both principals are retained independently so a future adapter
/// must explicitly check the subject join instead of assuming a Console grant creates a Harness
/// identity. This record is not serialized as a Harness request body.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerIdentityCorrelationV2 {
    pub console: ConsoleOwnerIdentityV2,
    pub console_scope: ConsoleOwnerScopeV2,
    pub harness: HarnessActorIdentityV2,
}

impl OwnerIdentityCorrelationV2 {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        self.console.validate()?;
        self.console_scope.validate()?;
        self.harness.validate()?;
        if self.console.subject != self.harness.actor_subject {
            return Err(OwnerWireError::CorrelationMismatch("actor_subject"));
        }
        if self.console_scope.run_id != self.harness.workflow_run_id {
            return Err(OwnerWireError::CorrelationMismatch("workflow_run_id"));
        }
        Ok(())
    }
}
