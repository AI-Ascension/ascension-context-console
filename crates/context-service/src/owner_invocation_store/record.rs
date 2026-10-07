use crate::authenticated_ingress::VerifiedPrincipal;
use crate::control::Scope;
use crate::harness_context_owner_wire::{
    ContextOwnerEndpointV1, ContextOwnerInvocationV2, HarnessResponseV1,
};
use crate::harness_facade::ProtectedAuthReference;
use crate::protected_owner_credentials::ResolvedOwnerCredential;
use crate::subject_grants::AdmittedSubjectGrant;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use super::operations::{grant_snapshot, required_console_permissions, required_harness_scopes};

#[cfg(test)]
#[path = "record/admission_tests.rs"]
mod admission_tests;
pub(super) const RECORD_SCHEMA_VERSION: u16 = 1;
pub(super) const MAX_ATTEMPTS: usize = 16;
pub(super) const MAX_AUTH_GRANTS: usize = 8;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(i64)]
pub(super) enum EntryState {
    Prepared = 0,
    WriteClaimed = 1,
    DefinitelyNotSent = 2,
    LookupClaimed = 3,
    Unknown = 4,
    Completed = 5,
}

impl EntryState {
    pub(super) fn from_sql(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Prepared),
            1 => Some(Self::WriteClaimed),
            2 => Some(Self::DefinitelyNotSent),
            3 => Some(Self::LookupClaimed),
            4 => Some(Self::Unknown),
            5 => Some(Self::Completed),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum LookupFamily {
    Mutation,
    Control,
    Publication,
    SourceUpload,
}

impl LookupFamily {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Mutation => "mutation",
            Self::Control => "control",
            Self::Publication => "publication",
            Self::SourceUpload => "source_upload",
        }
    }

    pub(super) fn has_exact_lookup(self) -> bool {
        matches!(self, Self::Mutation | Self::Control | Self::Publication)
    }
}

pub(super) struct StableLocator {
    pub family: LookupFamily,
    pub stable_key: String,
    pub issuer: String,
    pub subject: String,
    pub audience: String,
    pub scope: Scope,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GrantSnapshot {
    pub grant_id: String,
    pub issuer: String,
    pub subject: String,
    pub permission: String,
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
    pub not_before: u64,
    pub expires_at: u64,
    pub revocation_generation: u64,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionSnapshot {
    pub issuer: String,
    pub subject: String,
    pub audience: String,
    pub console_credential_id: String,
    pub principal_expires_at: u64,
    pub scope: Scope,
    pub harness_actor: String,
    pub harness_owner: String,
    pub harness_workflow_run: String,
    pub binding: Option<crate::harness_context_owner_wire::ContextOwnerBinding>,
    pub required_permissions: Vec<String>,
    pub harness_scopes: Vec<String>,
    pub grants: Vec<GrantSnapshot>,
    pub credential_reference_id: String,
    pub credential_expires_at: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AttemptKind {
    Write,
    Lookup,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttemptSnapshot {
    pub kind: AttemptKind,
    pub console_credential_id: String,
    pub principal_expires_at: u64,
    pub grants: Vec<GrantSnapshot>,
    pub required_permissions: Vec<String>,
    pub harness_scopes: Vec<String>,
    pub credential_reference_id: String,
    pub credential_expires_at: u64,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InvocationRecord {
    pub schema_version: u16,
    pub state: EntryState,
    pub sequence: u64,
    pub data_key_id: String,
    pub origin: AdmissionSnapshot,
    pub invocation: ContextOwnerInvocationV2,
    pub endpoint: ContextOwnerEndpointV1,
    /// Exact UTF-8 JSON request bytes represented as a string so the encrypted JSON envelope
    /// does not expand each byte into a decimal integer.
    pub canonical_body: String,
    pub attempts: Vec<AttemptSnapshot>,
    pub response: Option<HarnessResponseV1>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionUse {
    Write,
    ExactLookup,
    CachedRead,
    ReadOnly,
}

/// Crate-private snapshot that can only be made from the verified ingress objects.
/// Its fields and formatting stay private; no bearer or key bytes are accepted here.
pub(crate) struct TrustedInvocationAdmission {
    pub(super) snapshot: AdmissionSnapshot,
    pub(super) reference: ProtectedAuthReference,
    pub(super) invocation: ContextOwnerInvocationV2,
    pub(super) use_kind: AdmissionUse,
    monotonic_deadline: Instant,
}

impl TrustedInvocationAdmission {
    pub(crate) fn from_verified_ingress(
        admission_started: Instant,
        principal: &VerifiedPrincipal,
        grants: &[AdmittedSubjectGrant],
        owner: &ResolvedOwnerCredential,
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<Self, AdmissionError> {
        invocation.validate().map_err(|_| AdmissionError::Denied)?;
        let identity = &invocation.identity;
        let descriptor = owner.descriptor();
        if !descriptor.is_valid()
            || principal.expires_at() <= now
            || descriptor.expires_at() <= now
            || descriptor.expires_at() > principal.expires_at()
            || principal.issuer() != identity.console.issuer
            || principal.subject() != identity.console.subject
            || principal.audience() != identity.console.audience
            || principal.credential_id() != identity.console.credential_id
            || descriptor.console_issuer() != principal.issuer()
            || descriptor.console_subject() != principal.subject()
            || descriptor.harness_subject() != identity.harness.actor_subject
            || descriptor.scope().project_id != identity.console_scope.project_id
            || descriptor.scope().run_id != identity.console_scope.run_id
            || descriptor.scope().episode_id != identity.console_scope.episode_id
            || descriptor.scope().agent_id != identity.console_scope.agent_id
            || descriptor.expires_at() != identity.harness.credential_expires_at
            || owner.reference().as_str() != identity.harness.credential_reference_id
            || identity.harness.workflow_run_id != identity.console_scope.run_id
        {
            return Err(AdmissionError::Denied);
        }

        let scope = descriptor.scope().clone();
        let required = required_console_permissions(&invocation.operation, use_kind)?;
        if grants.is_empty() || grants.len() > MAX_AUTH_GRANTS {
            return Err(AdmissionError::Denied);
        }
        let mut earliest_expiry = principal.expires_at().min(descriptor.expires_at());
        let mut seen = BTreeSet::new();
        let mut grant_records = Vec::with_capacity(grants.len());
        for grant in grants {
            if grant.issuer != principal.issuer()
                || grant.subject != principal.subject()
                || grant.scope != scope
                || grant.not_before > now
                || grant.expires_at <= now
                || !seen.insert(grant.permission.as_str().to_owned())
                || !descriptor.permits_exact_scope(
                    crate::protected_owner_credentials::harness_scope_for(grant.permission),
                )
            {
                return Err(AdmissionError::Denied);
            }
            earliest_expiry = earliest_expiry.min(grant.expires_at);
            grant_records.push(grant_snapshot(grant));
        }
        if seen.len() != required.len()
            || required
                .iter()
                .any(|permission| !seen.contains(permission.as_str()))
            || !grants.iter().any(|grant| {
                grant.grant_id == identity.console.grant_id
                    && grant.revocation_generation == identity.console.grant_generation
                    && grant.expires_at == identity.console.grant_expires_at
            })
        {
            return Err(AdmissionError::Denied);
        }

        let mut harness_scopes = BTreeSet::new();
        for scope_name in required_harness_scopes(&invocation.operation, use_kind)? {
            if !descriptor.permits_exact_scope(scope_name) {
                return Err(AdmissionError::Denied);
            }
            harness_scopes.insert(scope_name.to_owned());
        }
        let snapshot = AdmissionSnapshot {
            issuer: principal.issuer().to_owned(),
            subject: principal.subject().to_owned(),
            audience: principal.audience().to_owned(),
            console_credential_id: principal.credential_id().to_owned(),
            principal_expires_at: principal.expires_at(),
            scope,
            harness_actor: identity.harness.actor_subject.clone(),
            harness_owner: identity.harness.owner_id.clone(),
            harness_workflow_run: identity.harness.workflow_run_id.clone(),
            binding: invocation.expected_binding.clone(),
            required_permissions: required
                .into_iter()
                .map(|permission| permission.as_str().to_owned())
                .collect(),
            harness_scopes: harness_scopes.into_iter().collect(),
            grants: grant_records,
            credential_reference_id: owner.reference().as_str().to_owned(),
            credential_expires_at: descriptor.expires_at(),
        };
        // `now` is an integer wall-clock timestamp, so subtract one second to avoid extending
        // authority past expiry due to its fractional part. `Instant` prevents wall-clock
        // rollback from extending this admission between reservation and transport claim.
        let remaining_seconds = earliest_expiry
            .checked_sub(now)
            .and_then(|seconds| seconds.checked_sub(1))
            .filter(|seconds| *seconds > 0)
            .ok_or(AdmissionError::Denied)?;
        if admission_started > Instant::now() {
            return Err(AdmissionError::Denied);
        }
        let monotonic_deadline = admission_started
            .checked_add(Duration::from_secs(remaining_seconds))
            .ok_or(AdmissionError::Denied)?;
        Ok(Self {
            snapshot,
            reference: owner.reference().clone(),
            invocation: invocation.clone(),
            use_kind,
            monotonic_deadline,
        })
    }

    pub(crate) fn validate_for(
        &self,
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<(), AdmissionError> {
        self.deadline_for(invocation, use_kind, now).map(|_| ())
    }

    pub(crate) fn deadline_for(
        &self,
        invocation: &ContextOwnerInvocationV2,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<Instant, AdmissionError> {
        if self.is_valid_at(now, use_kind) && &self.invocation == invocation {
            Ok(self.monotonic_deadline)
        } else {
            Err(AdmissionError::Denied)
        }
    }

    pub(super) fn is_valid_at(&self, now: u64, expected_use: AdmissionUse) -> bool {
        self.use_kind == expected_use
            && Instant::now() < self.monotonic_deadline
            && self.snapshot.principal_expires_at > now
            && self.snapshot.credential_expires_at > now
            && self
                .snapshot
                .grants
                .iter()
                .all(|grant| grant.not_before <= now && grant.expires_at > now)
    }

    pub(super) fn attempt(&self, kind: AttemptKind) -> AttemptSnapshot {
        AttemptSnapshot {
            kind,
            console_credential_id: self.snapshot.console_credential_id.clone(),
            principal_expires_at: self.snapshot.principal_expires_at,
            grants: self.snapshot.grants.clone(),
            required_permissions: self.snapshot.required_permissions.clone(),
            harness_scopes: self.snapshot.harness_scopes.clone(),
            credential_reference_id: self.snapshot.credential_reference_id.clone(),
            credential_expires_at: self.snapshot.credential_expires_at,
        }
    }
}

impl std::fmt::Debug for TrustedInvocationAdmission {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TrustedInvocationAdmission(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AdmissionError {
    Denied,
    UnsupportedOperation,
}
