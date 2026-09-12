// SPDX-License-Identifier: MIT

//! Bounded decoding of the harness-owned workflow-context association wire
//! contract. The reader exposes redacted metadata only and never follows a
//! location, reads a component, or grants context-control authority.

use crate::MAX_SNAPSHOT_BYTES;
use crate::json::{
    self, AccessError, Object, Value, bounded, field, id_field, keys, obj, optional_id,
    optional_number, strv, val,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssociationError {
    Empty,
    TooLarge,
    Invalid(&'static str),
}

impl AccessError for AssociationError {
    fn invalid(field: &'static str) -> Self {
        Self::Invalid(field)
    }
}

impl std::fmt::Display for AssociationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("workflow context association is empty"),
            Self::TooLarge => formatter.write_str("workflow context association exceeds its bound"),
            Self::Invalid(field) => write!(
                formatter,
                "workflow context association field is invalid: {field}"
            ),
        }
    }
}

impl std::error::Error for AssociationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowIdentity {
    pub workflow_run_id: String,
    pub definition_digest: String,
    pub graph_id: String,
    pub node_id: String,
    pub node_execution_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextBinding {
    pub availability: String,
    pub context_ref: Option<String>,
    pub run_id: Option<String>,
    pub episode_id: Option<String>,
    pub agent_id: Option<String>,
    pub snapshot_id: Option<String>,
    pub approved_revision_id: Option<String>,
    pub plan_epoch: Option<u64>,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureEvidence {
    pub mode: String,
    pub state: String,
    pub attempt_id: Option<String>,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InspectionCapabilities {
    pub inspect_metadata: bool,
    pub read_retained_content: bool,
    pub edit_context: bool,
    pub control_context: bool,
    pub memory_search: bool,
    pub provider_session_inspect: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowContextAssociation {
    pub workflow: WorkflowIdentity,
    pub context: ContextBinding,
    pub capture: CaptureEvidence,
    pub capabilities: InspectionCapabilities,
}

impl WorkflowContextAssociation {
    pub fn parse(bytes: &[u8]) -> Result<Self, AssociationError> {
        if bytes.is_empty() {
            return Err(AssociationError::Empty);
        }
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(AssociationError::TooLarge);
        }
        let root = json::parse(bytes).map_err(|_| AssociationError::Invalid("json"))?;
        let root = root.object().ok_or(AssociationError::Invalid("object"))?;
        keys::<AssociationError>(
            root,
            &[
                "schema_version",
                "workflow",
                "context",
                "capture",
                "capabilities",
            ],
        )?;
        if strv::<AssociationError>(root, "schema_version")?
            != "ascension.workflow-context-association/v1"
        {
            return Err(AssociationError::Invalid("schema_version"));
        }
        Ok(Self {
            workflow: workflow(obj::<AssociationError>(root, "workflow")?)?,
            context: context(obj::<AssociationError>(root, "context")?)?,
            capture: capture(obj::<AssociationError>(root, "capture")?)?,
            capabilities: capabilities(obj::<AssociationError>(root, "capabilities")?)?,
        })
    }
}

fn workflow(value: &Object) -> Result<WorkflowIdentity, AssociationError> {
    keys::<AssociationError>(
        value,
        &[
            "workflow_run_id",
            "definition_digest",
            "graph_id",
            "node_id",
            "node_execution_id",
        ],
    )?;
    let definition_digest = bounded::<AssociationError>(value, "definition_digest", 64)?;
    if definition_digest.len() != 64
        || !definition_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(AssociationError::Invalid("definition_digest"));
    }
    Ok(WorkflowIdentity {
        workflow_run_id: id_field::<AssociationError>(value, "workflow_run_id")?,
        definition_digest,
        graph_id: id_field::<AssociationError>(value, "graph_id")?,
        node_id: id_field::<AssociationError>(value, "node_id")?,
        node_execution_id: id_field::<AssociationError>(value, "node_execution_id")?,
    })
}

fn context(value: &Object) -> Result<ContextBinding, AssociationError> {
    required_with_optional(
        value,
        &[
            "availability",
            "context_ref",
            "run_id",
            "episode_id",
            "agent_id",
            "snapshot_id",
            "approved_revision_id",
            "plan_epoch",
        ],
        &["reason_code"],
    )?;
    let availability = strv::<AssociationError>(value, "availability")?;
    if !matches!(
        availability.as_str(),
        "available" | "unavailable" | "not_applicable"
    ) {
        return Err(AssociationError::Invalid("availability"));
    }
    let binding = ContextBinding {
        availability,
        context_ref: optional_id::<AssociationError>(value, "context_ref")?,
        run_id: optional_id::<AssociationError>(value, "run_id")?,
        episode_id: optional_id::<AssociationError>(value, "episode_id")?,
        agent_id: optional_id::<AssociationError>(value, "agent_id")?,
        snapshot_id: optional_id::<AssociationError>(value, "snapshot_id")?,
        approved_revision_id: optional_id::<AssociationError>(value, "approved_revision_id")?,
        plan_epoch: optional_number::<AssociationError>(value, "plan_epoch")?,
        reason_code: optional_bounded(value, "reason_code", 128)?,
    };
    let identifiers = [
        binding.context_ref.as_ref(),
        binding.run_id.as_ref(),
        binding.episode_id.as_ref(),
        binding.agent_id.as_ref(),
        binding.snapshot_id.as_ref(),
        binding.approved_revision_id.as_ref(),
    ];
    if binding.availability == "available" {
        if identifiers.iter().any(Option::is_none) || binding.plan_epoch.unwrap_or_default() == 0 {
            return Err(AssociationError::Invalid("available_context_binding"));
        }
    } else if identifiers.iter().any(Option::is_some) || binding.plan_epoch.is_some() {
        return Err(AssociationError::Invalid("unavailable_context_binding"));
    }
    Ok(binding)
}

fn capture(value: &Object) -> Result<CaptureEvidence, AssociationError> {
    required_with_optional(value, &["mode", "state", "attempt_id"], &["reason_code"])?;
    let mode = strv::<AssociationError>(value, "mode")?;
    let state = strv::<AssociationError>(value, "state")?;
    if !matches!(
        mode.as_str(),
        "off" | "metadata" | "memory" | "private" | "unavailable"
    ) || !matches!(
        state.as_str(),
        "not_captured"
            | "prepared"
            | "input_write_completed"
            | "provider_receipt_reported"
            | "unknown"
            | "unavailable"
    ) {
        return Err(AssociationError::Invalid("capture"));
    }
    Ok(CaptureEvidence {
        mode,
        state,
        attempt_id: optional_id::<AssociationError>(value, "attempt_id")?,
        reason_code: optional_bounded(value, "reason_code", 128)?,
    })
}

fn capabilities(value: &Object) -> Result<InspectionCapabilities, AssociationError> {
    keys::<AssociationError>(
        value,
        &[
            "inspect_metadata",
            "read_retained_content",
            "edit_context",
            "control_context",
            "memory_search",
            "provider_session_inspect",
        ],
    )?;
    Ok(InspectionCapabilities {
        inspect_metadata: boolean(value, "inspect_metadata")?,
        read_retained_content: boolean(value, "read_retained_content")?,
        edit_context: boolean(value, "edit_context")?,
        control_context: boolean(value, "control_context")?,
        memory_search: boolean(value, "memory_search")?,
        provider_session_inspect: boolean(value, "provider_session_inspect")?,
    })
}

fn boolean(value: &Object, name: &'static str) -> Result<bool, AssociationError> {
    val::<AssociationError>(value, name)?
        .boolean()
        .ok_or(AssociationError::Invalid(name))
}

fn optional_bounded(
    value: &Object,
    name: &'static str,
    limit: usize,
) -> Result<Option<String>, AssociationError> {
    match field(value, name) {
        None => Ok(None),
        Some(Value::String(text))
            if !text.is_empty() && text.len() <= limit && !text.chars().any(char::is_control) =>
        {
            Ok(Some(text.clone()))
        }
        _ => Err(AssociationError::Invalid(name)),
    }
}

fn required_with_optional(
    value: &Object,
    required: &[&str],
    optional: &[&str],
) -> Result<(), AssociationError> {
    if required.iter().any(|name| field(value, name).is_none())
        || value.iter().any(|(name, _)| {
            !required.contains(&name.as_str()) && !optional.contains(&name.as_str())
        })
    {
        return Err(AssociationError::Invalid("unknown or missing field"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSOCIATION: &[u8] = br#"{
      "schema_version":"ascension.workflow-context-association/v1",
      "workflow":{"workflow_run_id":"run.fixture.1","definition_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","graph_id":"main","node_id":"decide","node_execution_id":"run.fixture.1.node.2"},
      "context":{"availability":"available","context_ref":"context.fixture","run_id":"run.fixture.1","episode_id":"episode.fixture.1","agent_id":"agent.fixture.1","snapshot_id":"snapshot.fixture.2","approved_revision_id":"revision.fixture.1","plan_epoch":2},
      "capture":{"mode":"metadata","state":"prepared","attempt_id":"attempt.fixture.2"},
      "capabilities":{"inspect_metadata":true,"read_retained_content":false,"edit_context":false,"control_context":false,"memory_search":false,"provider_session_inspect":false}
    }"#;

    #[test]
    fn parses_a_complete_bound_association_without_retaining_content() {
        let value = WorkflowContextAssociation::parse(ASSOCIATION).expect("association parses");
        assert_eq!(value.workflow.node_execution_id, "run.fixture.1.node.2");
        assert_eq!(
            value.context.snapshot_id.as_deref(),
            Some("snapshot.fixture.2")
        );
        assert!(!value.capabilities.read_retained_content);
    }

    #[test]
    fn rejects_an_unavailable_association_that_invents_a_context_identity() {
        let text = String::from_utf8(ASSOCIATION.to_vec())
            .expect("fixture UTF-8")
            .replace(
                "\"availability\":\"available\"",
                "\"availability\":\"unavailable\"",
            );
        assert!(WorkflowContextAssociation::parse(text.as_bytes()).is_err());
    }
}
