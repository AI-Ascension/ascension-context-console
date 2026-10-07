use super::{AdmissionSnapshot, AdmissionUse, GrantSnapshot, TrustedInvocationAdmission};
use crate::control::Scope;
use crate::harness_context_owner_wire::ContextOwnerInvocationV2;
use crate::harness_facade::ProtectedAuthReference;
use std::time::{Duration, Instant};

use super::super::operations::{required_console_permissions, required_harness_scopes};

impl TrustedInvocationAdmission {
    pub(crate) fn synthetic(
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Self {
        let identity = &invocation.identity;
        let required = required_console_permissions(&invocation.operation, use_kind)
            .expect("supported synthetic test permission set");
        let harness_scopes = required_harness_scopes(&invocation.operation, use_kind)
            .expect("supported synthetic test scope set");
        let scope = Scope {
            project_id: identity.console_scope.project_id.clone(),
            run_id: identity.console_scope.run_id.clone(),
            episode_id: identity.console_scope.episode_id.clone(),
            agent_id: identity.console_scope.agent_id.clone(),
        };
        let grants = required
            .iter()
            .enumerate()
            .map(|(index, permission)| GrantSnapshot {
                grant_id: if index == 0 {
                    identity.console.grant_id.clone()
                } else {
                    format!("test-grant-{index}")
                },
                issuer: identity.console.issuer.clone(),
                subject: identity.console.subject.clone(),
                permission: permission.as_str().to_owned(),
                project_id: scope.project_id.clone(),
                run_id: scope.run_id.clone(),
                episode_id: scope.episode_id.clone(),
                agent_id: scope.agent_id.clone(),
                not_before: now.saturating_sub(1),
                expires_at: identity.console.grant_expires_at,
                revocation_generation: identity.console.grant_generation,
            })
            .collect();
        let snapshot = AdmissionSnapshot {
            issuer: identity.console.issuer.clone(),
            subject: identity.console.subject.clone(),
            audience: identity.console.audience.clone(),
            console_credential_id: identity.console.credential_id.clone(),
            principal_expires_at: identity.console.grant_expires_at,
            scope,
            harness_actor: identity.harness.actor_subject.clone(),
            harness_owner: identity.harness.owner_id.clone(),
            harness_workflow_run: identity.harness.workflow_run_id.clone(),
            binding: invocation.expected_binding.clone(),
            required_permissions: required
                .iter()
                .map(|permission| permission.as_str().to_owned())
                .collect(),
            harness_scopes: harness_scopes.into_iter().map(str::to_owned).collect(),
            grants,
            credential_reference_id: identity.harness.credential_reference_id.clone(),
            credential_expires_at: identity.harness.credential_expires_at,
        };
        Self {
            snapshot,
            reference: ProtectedAuthReference::new(
                identity.harness.credential_reference_id.clone(),
            )
            .expect("synthetic test credential reference"),
            invocation: invocation.clone(),
            use_kind,
            monotonic_deadline: Instant::now() + Duration::from_secs(60),
        }
    }

    pub(crate) fn synthetic_with_started(
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        now: u64,
        admission_started: Instant,
    ) -> Self {
        let mut admission = Self::synthetic(invocation, use_kind, now);
        admission.monotonic_deadline = admission_started
            .checked_add(Duration::from_secs(60))
            .expect("synthetic deadline");
        admission
    }

    pub(crate) fn synthetic_with_lifetime(
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        now: u64,
        lifetime: Duration,
    ) -> Self {
        let mut admission = Self::synthetic(invocation, use_kind, now);
        admission.monotonic_deadline = Instant::now()
            .checked_add(lifetime)
            .expect("synthetic deadline");
        admission
    }
}
