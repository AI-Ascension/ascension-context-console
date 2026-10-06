// SPDX-License-Identifier: MIT

//! Operator-owned resolution of a short-lived Harness owner capability.
//!
//! This module never stores or accepts a bearer credential. The resolver is responsible for
//! looking up an already protected local credential and returning only its opaque owner reference
//! plus an independently established description of the operations that credential can perform.

use crate::authenticated_ingress::VerifiedPrincipal;
use crate::control::Scope;
use crate::harness_facade::{FacadePermission, ProtectedAuthReference};
use std::collections::BTreeSet;

/// Exact Harness-side permission required for a Console operation.
///
/// This is intentionally an exact mapping. A Console adapter does not reinterpret Harness
/// wildcard scopes; the Harness remains responsible for enforcing the credential it receives.
pub(crate) fn harness_scope_for(permission: FacadePermission) -> &'static str {
    match permission {
        FacadePermission::MetadataRead => "workflow:read",
        FacadePermission::ContentRead => "workflow:context:content:read",
        FacadePermission::Edit => "workflow:context:edit",
        FacadePermission::Objective => "workflow:context:objective:edit",
        FacadePermission::Commit | FacadePermission::Pause | FacadePermission::Resume => {
            "workflow:control"
        }
    }
}

const MAX_DESCRIPTOR_SCOPES: usize = 5;
const KNOWN_HARNESS_SCOPES: &[&str] = &[
    "workflow:read",
    "workflow:context:content:read",
    "workflow:context:edit",
    "workflow:context:objective:edit",
    "workflow:control",
];

/// Independently resolved identity and scope facts for one protected owner reference.
#[derive(Clone, Eq, PartialEq)]
pub struct OwnerCredentialDescriptor {
    console_issuer: String,
    console_subject: String,
    harness_subject: String,
    scope: Scope,
    exact_harness_scopes: BTreeSet<String>,
    expires_at: u64,
}

impl OwnerCredentialDescriptor {
    /// Build a descriptor from operator-owned credential metadata.
    pub fn new(
        console_issuer: impl Into<String>,
        console_subject: impl Into<String>,
        harness_subject: impl Into<String>,
        scope: Scope,
        exact_harness_scopes: impl IntoIterator<Item = String>,
        expires_at: u64,
    ) -> Self {
        Self {
            console_issuer: console_issuer.into(),
            console_subject: console_subject.into(),
            harness_subject: harness_subject.into(),
            scope,
            exact_harness_scopes: exact_harness_scopes.into_iter().collect(),
            expires_at,
        }
    }

    pub fn console_issuer(&self) -> &str {
        &self.console_issuer
    }

    pub fn console_subject(&self) -> &str {
        &self.console_subject
    }

    pub fn harness_subject(&self) -> &str {
        &self.harness_subject
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub fn permits_exact_scope(&self, scope: &str) -> bool {
        self.exact_harness_scopes.contains(scope)
    }

    pub(crate) fn is_valid(&self) -> bool {
        valid_identity_text(&self.console_issuer)
            && crate::harness_facade::valid_id(&self.console_subject)
            && crate::harness_facade::valid_id(&self.harness_subject)
            && valid_identity_text(&self.harness_subject)
            && scope_valid(&self.scope)
            && self.expires_at > 0
            && !self.exact_harness_scopes.is_empty()
            && self.exact_harness_scopes.len() <= MAX_DESCRIPTOR_SCOPES
            && self
                .exact_harness_scopes
                .iter()
                .all(|scope| KNOWN_HARNESS_SCOPES.contains(&scope.as_str()))
    }
}

impl std::fmt::Debug for OwnerCredentialDescriptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OwnerCredentialDescriptor")
            .field("console_issuer", &self.console_issuer)
            .field("console_subject", &self.console_subject)
            .field("harness_subject", &self.harness_subject)
            .field("scope", &self.scope)
            .field("exact_harness_scopes", &self.exact_harness_scopes)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

fn valid_identity_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::authenticated_ingress::MAX_PRINCIPAL_CLAIM_BYTES
        && !value.contains('\0')
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn scope_valid(scope: &Scope) -> bool {
    [
        scope.project_id.as_str(),
        scope.run_id.as_str(),
        scope.episode_id.as_str(),
        scope.agent_id.as_str(),
    ]
    .iter()
    .all(|value| crate::harness_facade::valid_id(value))
}

/// A credential reference and its independently resolved, non-secret authority description.
#[derive(Clone, Eq, PartialEq)]
pub struct ResolvedOwnerCredential {
    reference: ProtectedAuthReference,
    descriptor: OwnerCredentialDescriptor,
}

impl ResolvedOwnerCredential {
    pub fn new(reference: ProtectedAuthReference, descriptor: OwnerCredentialDescriptor) -> Self {
        Self {
            reference,
            descriptor,
        }
    }

    pub(crate) fn reference(&self) -> &ProtectedAuthReference {
        &self.reference
    }

    pub(crate) fn descriptor(&self) -> &OwnerCredentialDescriptor {
        &self.descriptor
    }
}

impl std::fmt::Debug for ResolvedOwnerCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResolvedOwnerCredential")
            .field("reference", &"<opaque>")
            .field("descriptor", &self.descriptor)
            .finish()
    }
}

/// A trusted local resolver for the protected Harness credential associated with a principal.
pub trait ProtectedOwnerCredentialResolver {
    /// Resolve one reference for the exact requested owner scopes.
    ///
    /// Implementations must use operator-controlled credential metadata, not request fields or
    /// the Console bearer. The returned subject/scope/expiry/scopes are checked again by ingress.
    fn resolve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        requested_harness_scopes: &[&'static str],
        now: u64,
    ) -> Result<ResolvedOwnerCredential, CredentialResolutionError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialResolutionError {
    Unavailable,
    Denied,
    Invalid,
}

impl std::fmt::Display for CredentialResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "owner credential is unavailable",
            Self::Denied => "owner credential is not authorized",
            Self::Invalid => "owner credential metadata is invalid",
        })
    }
}

impl std::error::Error for CredentialResolutionError {}
