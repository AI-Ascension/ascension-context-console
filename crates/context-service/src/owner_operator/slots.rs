use crate::authenticated_ingress::VerifiedPrincipal;
use crate::control::Scope;
use crate::harness_context_owner_wire::ContextOwnerInvocationV2;
use crate::harness_facade::ProtectedAuthReference;
use crate::harness_owner_transport::{CredentialRedemptionError, HarnessCredentialRedeemer};
use crate::owner_invocation_store::{AdmissionUse, required_harness_scopes};
use crate::protected_owner_credentials::{
    CredentialResolutionError, OwnerCredentialDescriptor, ProtectedOwnerCredentialResolver,
    ResolvedOwnerCredential,
};
use owner_seed_sha2::{Digest, Sha256};
use serde::Deserialize;
use std::collections::BTreeSet;
use zeroize::Zeroizing;

use super::config::OperatorConfig;
use super::protected_files::{AdapterError, PrivateRoot, validate_name};

const SCHEMA: &str = "ascension.context-console.owner-slots.v1";
const MAX_REGISTRY_BYTES: usize = 128 * 1024;
const MAX_SLOTS: usize = 256;
const MAX_BEARER_BYTES: usize = 4 * 1024;
const KNOWN_SCOPES: &[&str] = &[
    "workflow:read",
    "workflow:context:content:read",
    "workflow:content:write",
    "workflow:context:edit",
    "workflow:context:objective:edit",
    "workflow:control",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    slots: Vec<Slot>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Slot {
    owner_id: String,
    reference: String,
    console_issuer: String,
    console_subject: String,
    console_audience: String,
    console_credential_id: String,
    revoked: bool,
    harness_subject: String,
    project_id: String,
    run_id: String,
    episode_id: String,
    agent_id: String,
    expires_at: u64,
    exact_harness_scopes: Vec<String>,
    bearer_file_ref: String,
    bearer_sha256: String,
}

pub(super) struct FileOwnerSlotResolver {
    root: PrivateRoot,
    owner_id: String,
    registry_ref: String,
    forbidden_refs: BTreeSet<String>,
}

impl FileOwnerSlotResolver {
    pub(super) fn new(config: &OperatorConfig) -> Self {
        Self {
            root: config.root.clone(),
            owner_id: config.owner_id.clone(),
            registry_ref: config.owner_slots_ref.clone(),
            forbidden_refs: config.protected_file_refs(),
        }
    }
}

impl ProtectedOwnerCredentialResolver for FileOwnerSlotResolver {
    fn resolve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        requested_harness_scopes: &[&'static str],
        now: u64,
    ) -> Result<ResolvedOwnerCredential, CredentialResolutionError> {
        validate_requested_scopes(requested_harness_scopes)
            .map_err(|_| CredentialResolutionError::Invalid)?;
        let registry = load_registry(&self.root, &self.registry_ref, &self.forbidden_refs)
            .map_err(map_resolution_error)?;
        let matches = registry
            .slots
            .iter()
            .filter(|slot| slot_matches_principal(slot, principal, scope, &self.owner_id));
        let mut selected = matches.collect::<Vec<_>>();
        if selected.len() != 1 {
            return Err(CredentialResolutionError::Denied);
        }
        let slot = selected.pop().ok_or(CredentialResolutionError::Denied)?;
        if slot.expires_at <= now
            || requested_harness_scopes.iter().any(|required| {
                !slot
                    .exact_harness_scopes
                    .iter()
                    .any(|scope| scope == required)
            })
        {
            return Err(CredentialResolutionError::Denied);
        }
        let reference = ProtectedAuthReference::new(slot.reference.clone())
            .map_err(|_| CredentialResolutionError::Invalid)?;
        let descriptor = slot_descriptor(slot).map_err(|_| CredentialResolutionError::Invalid)?;
        Ok(ResolvedOwnerCredential::new(reference, descriptor))
    }
}

pub(super) struct FileHarnessBearerRedeemer {
    root: PrivateRoot,
    owner_id: String,
    registry_ref: String,
    journal_manifest_ref: String,
    forbidden_refs: BTreeSet<String>,
}

impl FileHarnessBearerRedeemer {
    pub(super) fn new(config: &OperatorConfig) -> Self {
        Self {
            root: config.root.clone(),
            owner_id: config.owner_id.clone(),
            registry_ref: config.owner_slots_ref.clone(),
            journal_manifest_ref: config.journal_key_manifest_ref.clone(),
            forbidden_refs: config.protected_file_refs(),
        }
    }
}

impl HarnessCredentialRedeemer for FileHarnessBearerRedeemer {
    fn redeem(
        &mut self,
        reference: &ProtectedAuthReference,
        descriptor: &OwnerCredentialDescriptor,
        invocation: &ContextOwnerInvocationV2,
        required_scopes: &[&'static str],
        now: u64,
    ) -> Result<Zeroizing<Vec<u8>>, CredentialRedemptionError> {
        validate_requested_scopes(required_scopes)
            .map_err(|_| CredentialRedemptionError::Denied)?;
        invocation
            .validate()
            .map_err(|_| CredentialRedemptionError::Denied)?;
        if !scopes_match_operation(invocation, required_scopes) {
            return Err(CredentialRedemptionError::Denied);
        }
        if !descriptor.is_valid() || descriptor.expires_at() <= now {
            return Err(CredentialRedemptionError::Denied);
        }
        let registry = load_registry(&self.root, &self.registry_ref, &self.forbidden_refs)
            .map_err(map_redemption_error)?;
        let journal_refs = super::keys::journal_key_file_refs(
            &self.root,
            &self.journal_manifest_ref,
            &self.registry_ref,
            &self.forbidden_refs,
        )
        .map_err(|_| CredentialRedemptionError::Unavailable)?;
        let mut found = registry.slots.iter().filter(|slot| {
            slot.reference == reference.as_str()
                && slot_matches_invocation(slot, invocation, &self.owner_id)
        });
        let slot = found.next().ok_or(CredentialRedemptionError::Denied)?;
        if found.next().is_some()
            || slot.expires_at <= now
            || slot_descriptor(slot).map_err(|_| CredentialRedemptionError::Denied)? != *descriptor
            || journal_refs.contains(&slot.bearer_file_ref)
            || required_scopes.iter().any(|required| {
                !slot
                    .exact_harness_scopes
                    .iter()
                    .any(|scope| scope == required)
                    || !descriptor.permits_exact_scope(required)
            })
        {
            return Err(CredentialRedemptionError::Denied);
        }
        let bytes = self
            .root
            .read_file(&slot.bearer_file_ref, MAX_BEARER_BYTES)
            .map_err(map_redemption_error)?;
        if !valid_bearer(&bytes) || hex_digest(&bytes) != slot.bearer_sha256 {
            return Err(CredentialRedemptionError::Denied);
        }
        Ok(bytes)
    }
}

fn load_registry(
    root: &PrivateRoot,
    reference: &str,
    forbidden_refs: &BTreeSet<String>,
) -> Result<Registry, AdapterError> {
    let bytes = root.read_file(reference, MAX_REGISTRY_BYTES)?;
    let registry: Registry = serde_json::from_slice(&bytes).map_err(|_| AdapterError::Invalid)?;
    validate_registry(&registry, forbidden_refs)?;
    Ok(registry)
}

pub(super) fn bearer_file_refs(
    root: &PrivateRoot,
    registry_ref: &str,
    forbidden_refs: &BTreeSet<String>,
) -> Result<BTreeSet<String>, AdapterError> {
    let registry = load_registry(root, registry_ref, forbidden_refs)?;
    Ok(registry
        .slots
        .iter()
        .map(|slot| slot.bearer_file_ref.clone())
        .collect())
}

pub(super) fn startup_bearer_refs(
    config: &OperatorConfig,
) -> Result<(PrivateRoot, Vec<(String, String)>), AdapterError> {
    let registry = load_registry(
        &config.root,
        &config.owner_slots_ref,
        &config.protected_file_refs(),
    )?;
    let refs = registry
        .slots
        .iter()
        .map(|slot| (slot.bearer_file_ref.clone(), slot.bearer_sha256.clone()))
        .collect();
    Ok((config.root.clone(), refs))
}

fn validate_registry(
    registry: &Registry,
    forbidden_refs: &BTreeSet<String>,
) -> Result<(), AdapterError> {
    if registry.schema_version != SCHEMA
        || registry.slots.is_empty()
        || registry.slots.len() > MAX_SLOTS
    {
        return Err(AdapterError::Invalid);
    }
    let mut refs = BTreeSet::new();
    let mut bearer_refs = BTreeSet::new();
    for slot in &registry.slots {
        if !crate::harness_facade::valid_id(&slot.owner_id)
            || ProtectedAuthReference::new(slot.reference.clone()).is_err()
            || !valid_claim(&slot.console_issuer)
            || !crate::harness_facade::valid_id(&slot.console_subject)
            || !valid_claim(&slot.console_audience)
            || !valid_claim(&slot.console_credential_id)
            || !crate::harness_facade::valid_id(&slot.harness_subject)
            || Scope::new(
                slot.project_id.clone(),
                slot.run_id.clone(),
                slot.episode_id.clone(),
                slot.agent_id.clone(),
            )
            .is_err()
            || slot.expires_at == 0
            || !valid_scope_list(&slot.exact_harness_scopes)
            || validate_name(&slot.bearer_file_ref).is_err()
            || forbidden_refs.contains(&slot.bearer_file_ref)
            || !valid_digest(&slot.bearer_sha256)
            || !refs.insert(slot.reference.as_str())
            || !bearer_refs.insert(slot.bearer_file_ref.as_str())
        {
            return Err(AdapterError::Invalid);
        }
    }
    Ok(())
}

fn slot_matches_principal(
    slot: &Slot,
    principal: &VerifiedPrincipal,
    scope: &Scope,
    owner_id: &str,
) -> bool {
    slot.owner_id == owner_id
        && slot.console_issuer == principal.issuer()
        && slot.console_subject == principal.subject()
        && slot.console_audience == principal.audience()
        && slot.console_credential_id == principal.credential_id()
        && !slot.revoked
        && slot.harness_subject == principal.subject()
        && slot_scope(slot).as_ref() == Some(scope)
}

fn slot_matches_invocation(
    slot: &Slot,
    invocation: &ContextOwnerInvocationV2,
    owner_id: &str,
) -> bool {
    let identity = &invocation.identity;
    slot.owner_id == owner_id
        && slot.reference == identity.harness.credential_reference_id
        && slot.console_issuer == identity.console.issuer
        && slot.console_subject == identity.console.subject
        && slot.console_audience == identity.console.audience
        && slot.console_credential_id == identity.console.credential_id
        && !slot.revoked
        && slot.harness_subject == identity.harness.actor_subject
        && slot.expires_at == identity.harness.credential_expires_at
        && slot_scope(slot).as_ref().is_some_and(|scope| {
            scope.project_id == identity.console_scope.project_id
                && scope.run_id == identity.console_scope.run_id
                && scope.episode_id == identity.console_scope.episode_id
                && scope.agent_id == identity.console_scope.agent_id
        })
}

fn slot_scope(slot: &Slot) -> Option<Scope> {
    Scope::new(
        slot.project_id.clone(),
        slot.run_id.clone(),
        slot.episode_id.clone(),
        slot.agent_id.clone(),
    )
    .ok()
}

fn slot_descriptor(slot: &Slot) -> Result<OwnerCredentialDescriptor, AdapterError> {
    let scope = slot_scope(slot).ok_or(AdapterError::Invalid)?;
    Ok(OwnerCredentialDescriptor::new(
        slot.console_issuer.clone(),
        slot.console_subject.clone(),
        slot.harness_subject.clone(),
        scope,
        slot.exact_harness_scopes.clone(),
        slot.expires_at,
    ))
}

fn validate_requested_scopes(scopes: &[&str]) -> Result<(), AdapterError> {
    let mut seen = BTreeSet::new();
    if scopes.is_empty()
        || scopes.len() > KNOWN_SCOPES.len()
        || scopes
            .iter()
            .any(|scope| !KNOWN_SCOPES.contains(scope) || !seen.insert(*scope))
    {
        Err(AdapterError::Invalid)
    } else {
        Ok(())
    }
}

fn scopes_match_operation(invocation: &ContextOwnerInvocationV2, supplied: &[&str]) -> bool {
    [
        AdmissionUse::ReadOnly,
        AdmissionUse::Write,
        AdmissionUse::ExactLookup,
    ]
    .into_iter()
    .any(|use_kind| {
        required_harness_scopes(&invocation.operation, use_kind)
            .is_ok_and(|expected| expected.iter().copied().eq(supplied.iter().copied()))
    })
}

fn valid_scope_list(scopes: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    !scopes.is_empty()
        && scopes.len() <= KNOWN_SCOPES.len()
        && scopes
            .iter()
            .all(|scope| KNOWN_SCOPES.contains(&scope.as_str()) && seen.insert(scope.as_str()))
}

fn valid_claim(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::authenticated_ingress::MAX_PRINCIPAL_CLAIM_BYTES
        && !value.contains('\0')
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub(super) fn valid_bearer(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes.len() <= MAX_BEARER_BYTES
        && bytes.iter().all(|byte| (0x21..=0x7e).contains(byte))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("writing to String");
    }
    encoded
}

fn map_resolution_error(error: AdapterError) -> CredentialResolutionError {
    match error {
        AdapterError::Invalid => CredentialResolutionError::Invalid,
        AdapterError::Denied => CredentialResolutionError::Denied,
        AdapterError::UnsupportedPlatform
        | AdapterError::Unavailable
        | AdapterError::KeyUnavailable => CredentialResolutionError::Unavailable,
    }
}

fn map_redemption_error(error: AdapterError) -> CredentialRedemptionError {
    match error {
        AdapterError::Invalid | AdapterError::Denied => CredentialRedemptionError::Denied,
        AdapterError::UnsupportedPlatform
        | AdapterError::Unavailable
        | AdapterError::KeyUnavailable => CredentialRedemptionError::Unavailable,
    }
}
