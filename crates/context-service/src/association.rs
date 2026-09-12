// SPDX-License-Identifier: MIT

//! Resolves a harness-owned association only against an already authorized,
//! retained snapshot. This module never follows a location or fills a missing
//! identity from a workflow run identifier.

use std::time::SystemTime;

use context_reader::{
    AssociationError, Snapshot, SnapshotProjection, WorkflowContextAssociation,
    parse_workflow_context_association,
};

use crate::{ReadError, ReadGrant, Store};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssociationResolutionError {
    InvalidAssociation,
    ContextUnavailable,
    SnapshotUnavailable,
    IdentityMismatch,
}

impl From<AssociationError> for AssociationResolutionError {
    fn from(_: AssociationError) -> Self {
        Self::InvalidAssociation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedWorkflowContext {
    pub association: WorkflowContextAssociation,
    pub snapshot: SnapshotProjection,
}

pub fn resolve_workflow_context_association(
    store: &Store,
    token: &[u8],
    grant: &ReadGrant,
    association_bytes: &[u8],
    now: SystemTime,
) -> Result<ResolvedWorkflowContext, AssociationResolutionError> {
    let association = parse_workflow_context_association(association_bytes)?;
    if association.context.availability != "available" {
        return Err(AssociationResolutionError::ContextUnavailable);
    }
    let snapshot_id = association
        .context
        .snapshot_id
        .as_deref()
        .ok_or(AssociationResolutionError::InvalidAssociation)?;
    let bytes = store
        .get(token, grant, snapshot_id, now)
        .map_err(map_read_error)?;
    let snapshot =
        Snapshot::parse(&bytes).map_err(|_| AssociationResolutionError::SnapshotUnavailable)?;
    let identity = &snapshot.projection().identity;
    if snapshot.projection().snapshot_id != snapshot_id
        || association.context.run_id.as_deref() != Some(identity.run_id.as_str())
        || association.context.episode_id.as_deref() != Some(identity.episode_id.as_str())
        || association.context.agent_id.as_deref() != Some(identity.agent_id.as_str())
        || association.capture.attempt_id.as_deref() != Some(identity.provider_attempt_id.as_str())
    {
        return Err(AssociationResolutionError::IdentityMismatch);
    }
    Ok(ResolvedWorkflowContext {
        association,
        snapshot: snapshot.projection().clone(),
    })
}

fn map_read_error(error: ReadError) -> AssociationResolutionError {
    match error {
        ReadError::NotFound
        | ReadError::InvalidScope
        | ReadError::Forbidden
        | ReadError::Expired => AssociationResolutionError::SnapshotUnavailable,
        ReadError::InvalidToken | ReadError::TooLarge => {
            AssociationResolutionError::InvalidAssociation
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;
    use crate::{CapturePrivilege, ReadGrant};

    const SNAPSHOT: &[u8] = include_bytes!("../../../fixtures/synthetic/snapshot.json");

    fn association(snapshot_id: &str, attempt_id: &str) -> Vec<u8> {
        format!(
            r#"{{"schema_version":"ascension.workflow-context-association/v1","workflow":{{"workflow_run_id":"workflow.demo.1","definition_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","graph_id":"main","node_id":"decide","node_execution_id":"workflow.demo.1.node.2"}},"context":{{"availability":"available","context_ref":"context.demo.v1","run_id":"run-t02-001","episode_id":"episode-t02-001","agent_id":"agent-t02-reader","snapshot_id":"{snapshot_id}","approved_revision_id":"revision.demo.1","plan_epoch":1}},"capture":{{"mode":"metadata","state":"prepared","attempt_id":"{attempt_id}"}},"capabilities":{{"inspect_metadata":true,"read_retained_content":false,"edit_context":false,"control_context":false,"memory_search":false,"provider_session_inspect":false}}}}"#
        ).into_bytes()
    }

    fn grant() -> ReadGrant {
        ReadGrant::issue(
            b"association-token",
            "agent-t02-reader",
            Some("run-t02-001".to_owned()),
            CapturePrivilege::Metadata,
            Duration::from_secs(60),
            UNIX_EPOCH,
        )
        .expect("grant")
    }

    #[test]
    fn resolves_only_an_exact_scoped_snapshot_identity() {
        let mut store = Store::default();
        store.ingest(SNAPSHOT).expect("snapshot ingests");
        let resolved = resolve_workflow_context_association(
            &store,
            b"association-token",
            &grant(),
            &association("snapshot-t02-synthetic-001", "attempt-t02-001"),
            UNIX_EPOCH,
        )
        .expect("exact association resolves");
        assert_eq!(resolved.snapshot.snapshot_id, "snapshot-t02-synthetic-001");

        assert_eq!(
            resolve_workflow_context_association(
                &store,
                b"association-token",
                &grant(),
                &association("snapshot-t02-synthetic-001", "attempt.other"),
                UNIX_EPOCH,
            ),
            Err(AssociationResolutionError::IdentityMismatch),
        );
    }

    #[test]
    fn unavailable_and_out_of_scope_associations_never_fall_back_to_a_run_match() {
        let mut store = Store::default();
        store.ingest(SNAPSHOT).expect("snapshot ingests");
        let unavailable = br#"{
          "schema_version":"ascension.workflow-context-association/v1",
          "workflow":{"workflow_run_id":"workflow.demo.1","definition_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","graph_id":"main","node_id":"observe","node_execution_id":"workflow.demo.1.node.1"},
          "context":{"availability":"unavailable","context_ref":null,"run_id":null,"episode_id":null,"agent_id":null,"snapshot_id":null,"approved_revision_id":null,"plan_epoch":null},
          "capture":{"mode":"unavailable","state":"unavailable","attempt_id":null},
          "capabilities":{"inspect_metadata":true,"read_retained_content":false,"edit_context":false,"control_context":false,"memory_search":false,"provider_session_inspect":false}
        }"#;
        assert_eq!(
            resolve_workflow_context_association(
                &store,
                b"association-token",
                &grant(),
                unavailable,
                UNIX_EPOCH,
            ),
            Err(AssociationResolutionError::ContextUnavailable),
        );

        let wrong_scope = ReadGrant::issue(
            b"association-token",
            "agent-other",
            Some("run-t02-001".to_owned()),
            CapturePrivilege::Metadata,
            Duration::from_secs(60),
            UNIX_EPOCH,
        )
        .expect("grant");
        assert_eq!(
            resolve_workflow_context_association(
                &store,
                b"association-token",
                &wrong_scope,
                &association("snapshot-t02-synthetic-001", "attempt-t02-001"),
                UNIX_EPOCH,
            ),
            Err(AssociationResolutionError::SnapshotUnavailable),
        );
    }
}
