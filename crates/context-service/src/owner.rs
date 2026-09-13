// SPDX-License-Identifier: MIT

//! Bounded, target-owned port for delegating memory and provider-session work to the harness.
//!
//! The real harness implementation is intentionally not a dependency of this crate.  The
//! [`HarnessOwner`] trait is the versioned seam that an integration supplies at composition time.
//! The console validates the request and the public response envelope, while source admission,
//! retrieval, generation, session state, compaction, credentials and game authority remain on the
//! other side of the port.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const OWNER_RECEIPT_SCHEMA: &str = "ascension.harness-owner.receipt.v1";
pub const OWNER_MAX_BODY_BYTES: usize = 16 * 1024;
pub const OWNER_MAX_RESPONSE_BYTES: usize = 16 * 1024;
pub const OWNER_MAX_RESPONSE_DEPTH: usize = 8;

/// The three independent authority lanes exposed by the attached composition.
///
/// A grant is valid for exactly one lane.  In particular, a read/search grant does not imply
/// generation/review or control authority, and a generation/review grant does not imply control.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerGrantClass {
    ReadSearch,
    GenerationReview,
    Control,
}

impl OwnerGrantClass {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadSearch => "read_search",
            Self::GenerationReview => "generation_review",
            Self::Control => "control",
        }
    }
}

/// Scope shared by the memory and provider-session owner ports.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerScope {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

impl OwnerScope {
    /// Creates a scope after applying the same bounded identifier rules as the public routes.
    pub fn new(
        project_id: impl Into<String>,
        run_id: impl Into<String>,
        episode_id: impl Into<String>,
        agent_id: impl Into<String>,
    ) -> Result<Self, OwnerConfigError> {
        let scope = Self {
            project_id: project_id.into(),
            run_id: run_id.into(),
            episode_id: episode_id.into(),
            agent_id: agent_id.into(),
        };
        if [
            &scope.project_id,
            &scope.run_id,
            &scope.episode_id,
            &scope.agent_id,
        ]
        .iter()
        .all(|value| valid_id(value))
        {
            Ok(scope)
        } else {
            Err(OwnerConfigError::InvalidScope)
        }
    }

    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        self == other
    }

    #[must_use]
    pub fn is_valid(&self) -> bool {
        [
            &self.project_id,
            &self.run_id,
            &self.episode_id,
            &self.agent_id,
        ]
        .iter()
        .all(|value| valid_id(value))
    }
}

/// Request security context supplied by the authenticated console surface.
///
/// The context contains no provider credential. `principal` is an opaque grant identifier, not a
/// provider token. The optional CSRF value is accepted only for a configured control or
/// generation/review grant and is never included in an owner call.
#[derive(Clone, Eq, PartialEq)]
pub struct OwnerRequestContext {
    pub principal: String,
    pub host: String,
    pub origin: Option<String>,
    pub csrf_token: Option<String>,
    pub now: u64,
}

impl fmt::Debug for OwnerRequestContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerRequestContext")
            .field("principal", &self.principal)
            .field("host", &self.host)
            .field("origin", &self.origin)
            .field("csrf_configured", &self.csrf_token.is_some())
            .field("now", &self.now)
            .finish()
    }
}

impl OwnerRequestContext {
    #[must_use]
    pub fn new(
        principal: impl Into<String>,
        host: impl Into<String>,
        origin: Option<String>,
        csrf_token: Option<String>,
        now: u64,
    ) -> Self {
        Self {
            principal: principal.into(),
            host: host.into(),
            origin,
            csrf_token,
            now,
        }
    }
}

/// A short-lived, scoped grant for one owner authority lane.
#[derive(Clone, Eq, PartialEq)]
pub struct OwnerGrant {
    id: String,
    class: OwnerGrantClass,
    scope: OwnerScope,
    expires_at: u64,
    host: String,
    origin: Option<String>,
    csrf_token: Option<String>,
    revoked: bool,
}

impl fmt::Debug for OwnerGrant {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerGrant")
            .field("id", &self.id)
            .field("class", &self.class)
            .field("scope", &self.scope)
            .field("expires_at", &self.expires_at)
            .field("host", &self.host)
            .field("origin_configured", &self.origin.is_some())
            .field("csrf_configured", &self.csrf_token.is_some())
            .field("revoked", &self.revoked)
            .finish()
    }
}

impl OwnerGrant {
    /// Constructs a grant. The expiry is an absolute Unix-second timestamp.
    pub fn new(
        id: impl Into<String>,
        class: OwnerGrantClass,
        scope: OwnerScope,
        expires_at: u64,
        host: impl Into<String>,
        origin: Option<String>,
        csrf_token: Option<String>,
    ) -> Result<Self, OwnerConfigError> {
        let id = id.into();
        let host = host.into();
        if !valid_id(&id)
            || !scope.is_valid()
            || host.is_empty()
            || host.len() > 256
            || origin
                .as_deref()
                .is_some_and(|value| value.is_empty() || value.len() > 512)
            || csrf_token
                .as_deref()
                .is_some_and(|value| value.is_empty() || value.len() > 256)
            || expires_at == 0
            || class != OwnerGrantClass::ReadSearch && csrf_token.is_none()
        {
            return Err(OwnerConfigError::InvalidGrant);
        }
        Ok(Self {
            id,
            class,
            scope,
            expires_at,
            host,
            origin,
            csrf_token,
            revoked: false,
        })
    }

    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[must_use]
    pub const fn class(&self) -> OwnerGrantClass {
        self.class
    }

    #[must_use]
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }

    #[must_use]
    pub fn scope(&self) -> &OwnerScope {
        &self.scope
    }

    #[must_use]
    pub fn revoked(&self) -> bool {
        self.revoked
    }
}

/// Mutable grant book shared by routes built from one composition.
#[derive(Clone, Debug, Default)]
pub struct OwnerGrantBook {
    grants: BTreeMap<String, OwnerGrant>,
    revocation_epoch: u64,
}

impl OwnerGrantBook {
    #[must_use]
    pub fn new(grants: impl IntoIterator<Item = OwnerGrant>) -> Self {
        let mut book = Self::default();
        for grant in grants {
            book.grants.insert(grant.id.clone(), grant);
        }
        book
    }

    /// Fallible constructor for configuration loaders that must reject duplicate grant IDs.
    pub fn try_new(grants: impl IntoIterator<Item = OwnerGrant>) -> Result<Self, OwnerConfigError> {
        let mut book = Self::default();
        for grant in grants {
            book.insert(grant)?;
        }
        Ok(book)
    }

    pub fn insert(&mut self, grant: OwnerGrant) -> Result<(), OwnerConfigError> {
        if self.grants.contains_key(grant.id()) {
            return Err(OwnerConfigError::DuplicateGrant);
        }
        self.grants.insert(grant.id.clone(), grant);
        Ok(())
    }

    pub fn revoke(&mut self, grant_id: &str) -> Result<(), OwnerAuthError> {
        let grant = self
            .grants
            .get_mut(grant_id)
            .ok_or(OwnerAuthError::UnknownGrant)?;
        if !grant.revoked {
            grant.revoked = true;
            self.revocation_epoch = self.revocation_epoch.saturating_add(1);
        }
        Ok(())
    }

    #[must_use]
    pub const fn revocation_epoch(&self) -> u64 {
        self.revocation_epoch
    }

    fn authorize(
        &self,
        context: &OwnerRequestContext,
        class: OwnerGrantClass,
        scope: &OwnerScope,
        write: bool,
    ) -> Result<AuthorizedGrant, OwnerAuthError> {
        let grant = self
            .grants
            .get(&context.principal)
            .ok_or(OwnerAuthError::UnknownGrant)?;
        if grant.revoked {
            return Err(OwnerAuthError::Revoked);
        }
        if grant.expires_at <= context.now {
            return Err(OwnerAuthError::Expired);
        }
        if grant.class != class {
            return Err(OwnerAuthError::WrongGrantClass);
        }
        if !grant.scope.matches(scope) {
            return Err(OwnerAuthError::ScopeMismatch);
        }
        if context.host != grant.host {
            return Err(OwnerAuthError::HostMismatch);
        }
        if grant.origin.as_deref() != context.origin.as_deref() {
            return Err(OwnerAuthError::OriginMismatch);
        }
        if write && grant.csrf_token.as_deref() != context.csrf_token.as_deref() {
            return Err(OwnerAuthError::CsrfMismatch);
        }
        Ok(AuthorizedGrant {
            id: grant.id.clone(),
            class: grant.class,
            revocation_epoch: self.revocation_epoch,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AuthorizedGrant {
    id: String,
    class: OwnerGrantClass,
    revocation_epoch: u64,
}

/// Explicit composition point for an attached production owner.
///
/// Constructing this type is the only way for the target routes to enter delegation mode.
/// `OwnerGrantBook` is shared by clones so expiry/revocation decisions apply to every route built
/// from the composition. It is deliberately not serializable.
#[derive(Clone)]
pub struct HarnessOwnerComposition {
    owner: Arc<dyn HarnessOwner>,
    grants: Arc<Mutex<OwnerGrantBook>>,
}

impl fmt::Debug for HarnessOwnerComposition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HarnessOwnerComposition")
            .field("configured", &true)
            .finish()
    }
}

impl HarnessOwnerComposition {
    #[must_use]
    pub fn new(owner: Arc<dyn HarnessOwner>, grants: OwnerGrantBook) -> Self {
        Self {
            owner,
            grants: Arc::new(Mutex::new(grants)),
        }
    }

    #[must_use]
    pub fn owner(&self) -> Arc<dyn HarnessOwner> {
        Arc::clone(&self.owner)
    }

    pub fn revoke(&self, grant_id: &str) -> Result<(), OwnerAuthError> {
        self.grants
            .lock()
            .map_err(|_| OwnerAuthError::GrantBookUnavailable)?
            .revoke(grant_id)
    }

    #[must_use]
    pub fn revocation_epoch(&self) -> Option<u64> {
        self.grants.lock().ok().map(|book| book.revocation_epoch())
    }

    pub(crate) fn authorize(
        &self,
        context: &OwnerRequestContext,
        class: OwnerGrantClass,
        scope: &OwnerScope,
        write: bool,
    ) -> Result<OwnerGrantReceipt, OwnerAuthError> {
        let grant = self
            .grants
            .lock()
            .map_err(|_| OwnerAuthError::GrantBookUnavailable)?
            .authorize(context, class, scope, write)?;
        Ok(OwnerGrantReceipt {
            grant_id: grant.id,
            class: grant.class,
            revocation_epoch: grant.revocation_epoch,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerGrantReceipt {
    pub grant_id: String,
    pub class: OwnerGrantClass,
    pub revocation_epoch: u64,
}

/// Stable operation identifiers carried over the target-to-harness port.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerOperation {
    MemoryCapabilities,
    MemoryStatus,
    MemoryQuery,
    MemorySelection,
    MemoryGeneration,
    MemoryReview,
    SessionCapabilities,
    SessionStatus,
    SessionBinding,
    SessionList,
    SessionHistory,
    SessionEvents,
    SessionOperation,
    SessionCandidate,
    SessionHistoryRefresh,
    SessionReconnect,
    SessionForkPlan,
    SessionCompactionPlan,
    SessionPreparedBinding,
    SessionFork,
    SessionCompaction,
    SessionRetire,
    SessionCleanup,
}

impl OwnerOperation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MemoryCapabilities => "memory.capabilities",
            Self::MemoryStatus => "memory.status",
            Self::MemoryQuery => "memory.query",
            Self::MemorySelection => "memory.selection",
            Self::MemoryGeneration => "memory.generation",
            Self::MemoryReview => "memory.review",
            Self::SessionCapabilities => "provider_session.capabilities",
            Self::SessionStatus => "provider_session.status",
            Self::SessionBinding => "provider_session.binding",
            Self::SessionList => "provider_session.list",
            Self::SessionHistory => "provider_session.history",
            Self::SessionEvents => "provider_session.events",
            Self::SessionOperation => "provider_session.operation",
            Self::SessionCandidate => "provider_session.candidate",
            Self::SessionHistoryRefresh => "provider_session.history_refresh",
            Self::SessionReconnect => "provider_session.reconnect",
            Self::SessionForkPlan => "provider_session.fork_plan",
            Self::SessionCompactionPlan => "provider_session.compaction_plan",
            Self::SessionPreparedBinding => "provider_session.prepared_binding",
            Self::SessionFork => "provider_session.fork",
            Self::SessionCompaction => "provider_session.compaction",
            Self::SessionRetire => "provider_session.retire",
            Self::SessionCleanup => "provider_session.cleanup",
        }
    }

    #[must_use]
    pub const fn grant_class(self) -> OwnerGrantClass {
        match self {
            Self::MemoryCapabilities
            | Self::MemoryStatus
            | Self::MemoryQuery
            | Self::SessionCapabilities
            | Self::SessionStatus
            | Self::SessionBinding
            | Self::SessionList
            | Self::SessionHistory
            | Self::SessionEvents
            | Self::SessionOperation => OwnerGrantClass::ReadSearch,
            Self::MemoryGeneration | Self::MemoryReview => OwnerGrantClass::GenerationReview,
            Self::MemorySelection
            | Self::SessionCandidate
            | Self::SessionHistoryRefresh
            | Self::SessionReconnect
            | Self::SessionForkPlan
            | Self::SessionCompactionPlan
            | Self::SessionPreparedBinding
            | Self::SessionFork
            | Self::SessionCompaction
            | Self::SessionRetire
            | Self::SessionCleanup => OwnerGrantClass::Control,
        }
    }

    #[must_use]
    pub const fn read_only(self) -> bool {
        matches!(
            self,
            Self::MemoryCapabilities
                | Self::MemoryStatus
                | Self::MemoryQuery
                | Self::SessionCapabilities
                | Self::SessionStatus
                | Self::SessionBinding
                | Self::SessionList
                | Self::SessionHistory
                | Self::SessionEvents
                | Self::SessionOperation
        )
    }
}

/// A validated call. The payload is the closed, bounded JSON object from the public route.
pub struct OwnerCall {
    pub operation: OwnerOperation,
    pub scope: OwnerScope,
    pub reference: Option<String>,
    pub payload: Value,
    pub request_sha256: String,
    pub grant: OwnerGrantReceipt,
}

impl fmt::Debug for OwnerCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerCall")
            .field("operation", &self.operation)
            .field("scope", &self.scope)
            .field("reference", &self.reference)
            .field("request_sha256", &self.request_sha256)
            .field("grant", &self.grant)
            .finish_non_exhaustive()
    }
}

/// Lookup key used after a lost reply. A lookup never repeats the original operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerReceiptLookup {
    pub operation: OwnerOperation,
    pub scope: OwnerScope,
    pub reference: Option<String>,
    pub receipt_id: String,
}

/// Normalized owner outcome. Unknown means the operation may have taken effect and must not be
/// retried by this facade.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerOutcome {
    Accepted,
    Unknown,
    Unsupported,
}

impl OwnerOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Unknown => "unknown",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Receipt metadata returned by a harness owner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub operation_id: String,
    pub operation: String,
    pub source: String,
    pub owner_epoch: u64,
    pub evidence: String,
    pub outcome: OwnerOutcome,
    pub effect_applied: bool,
}

impl OwnerReceipt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        receipt_id: impl Into<String>,
        operation_id: impl Into<String>,
        operation: OwnerOperation,
        source: impl Into<String>,
        owner_epoch: u64,
        evidence: impl Into<String>,
        outcome: OwnerOutcome,
        effect_applied: bool,
    ) -> Result<Self, OwnerResponseError> {
        let receipt = Self {
            schema: OWNER_RECEIPT_SCHEMA.to_owned(),
            receipt_id: receipt_id.into(),
            operation_id: operation_id.into(),
            operation: operation.as_str().to_owned(),
            source: source.into(),
            owner_epoch,
            evidence: evidence.into(),
            outcome,
            effect_applied,
        };
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}

/// Result returned by [`HarnessOwner`].
#[derive(Clone, Eq, PartialEq)]
pub struct OwnerReply {
    pub receipt: OwnerReceipt,
    pub value: Option<Value>,
}

impl fmt::Debug for OwnerReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerReply")
            .field("receipt", &self.receipt)
            .field("value_present", &self.value.is_some())
            .finish()
    }
}

impl OwnerReply {
    pub fn new(receipt: OwnerReceipt, value: Option<Value>) -> Result<Self, OwnerResponseError> {
        validate_receipt(&receipt)?;
        if let Some(value) = &value {
            validate_public_value(value)?;
        }
        if receipt.outcome != OwnerOutcome::Accepted && value.is_some() {
            return Err(OwnerResponseError::ValueForNonAccepted);
        }
        Ok(Self { receipt, value })
    }

    /// Validates that a reply belongs to the operation that was requested and that read lanes
    /// remain effect-free.
    pub fn validate_for(&self, operation: OwnerOperation) -> Result<(), OwnerResponseError> {
        validate_receipt(&self.receipt)?;
        if self.receipt.operation != operation.as_str() {
            return Err(OwnerResponseError::OperationMismatch);
        }
        if let Some(value) = &self.value {
            validate_public_value(value)?;
        }
        if self.receipt.outcome != OwnerOutcome::Accepted && self.value.is_some() {
            return Err(OwnerResponseError::ValueForNonAccepted);
        }
        if operation.read_only()
            && (self.receipt.effect_applied || self.value.as_ref().is_some_and(nonzero_effect))
        {
            return Err(OwnerResponseError::ReadHadEffects);
        }
        Ok(())
    }

    pub fn unknown(
        receipt_id: impl Into<String>,
        operation_id: impl Into<String>,
        operation: OwnerOperation,
        source: impl Into<String>,
        owner_epoch: u64,
        evidence: impl Into<String>,
    ) -> Result<Self, OwnerResponseError> {
        Self::new(
            OwnerReceipt::new(
                receipt_id,
                operation_id,
                operation,
                source,
                owner_epoch,
                evidence,
                OwnerOutcome::Unknown,
                false,
            )?,
            None,
        )
    }

    pub fn unsupported(
        receipt_id: impl Into<String>,
        operation_id: impl Into<String>,
        operation: OwnerOperation,
        source: impl Into<String>,
        owner_epoch: u64,
        evidence: impl Into<String>,
    ) -> Result<Self, OwnerResponseError> {
        Self::new(
            OwnerReceipt::new(
                receipt_id,
                operation_id,
                operation,
                source,
                owner_epoch,
                evidence,
                OwnerOutcome::Unsupported,
                false,
            )?,
            None,
        )
    }
}

/// Errors at the transport/owner boundary. Details are intentionally not carried into API
/// diagnostics, avoiding credential, URL, path or private-content leakage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OwnerError {
    Unavailable,
    Unsupported,
    LostReply { receipt_id: String },
    UnknownReceipt,
    MalformedResponse,
}

impl fmt::Display for OwnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "harness owner is unavailable",
            Self::Unsupported => "harness owner does not support this operation",
            Self::LostReply { .. } => "harness owner reply was lost",
            Self::UnknownReceipt => "harness owner receipt is unknown",
            Self::MalformedResponse => "harness owner response is malformed",
        })
    }
}

impl std::error::Error for OwnerError {}

/// Errors while configuring grant composition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerConfigError {
    InvalidScope,
    InvalidGrant,
    DuplicateGrant,
}

impl fmt::Display for OwnerConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidScope => "owner scope is invalid",
            Self::InvalidGrant => "owner grant is invalid",
            Self::DuplicateGrant => "owner grant is duplicated",
        })
    }
}

impl std::error::Error for OwnerConfigError {}

/// Errors raised while authenticating an attached route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerAuthError {
    UnknownGrant,
    Revoked,
    Expired,
    WrongGrantClass,
    ScopeMismatch,
    HostMismatch,
    OriginMismatch,
    CsrfMismatch,
    GrantBookUnavailable,
}

impl fmt::Display for OwnerAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnknownGrant => "owner grant is unknown",
            Self::Revoked => "owner grant is revoked",
            Self::Expired => "owner grant is expired",
            Self::WrongGrantClass => "owner grant class is not authorized",
            Self::ScopeMismatch => "owner grant scope is not authorized",
            Self::HostMismatch => "owner host is not authorized",
            Self::OriginMismatch => "owner origin is not authorized",
            Self::CsrfMismatch => "owner csrf token is not authorized",
            Self::GrantBookUnavailable => "owner grant book is unavailable",
        })
    }
}

impl std::error::Error for OwnerAuthError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerResponseError {
    InvalidScope,
    InvalidReceipt,
    ValueForNonAccepted,
    ResponseTooLarge,
    ResponseTooDeep,
    ForbiddenField,
    OperationMismatch,
    ReadHadEffects,
}

impl fmt::Display for OwnerResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidScope => "harness owner scope is invalid",
            Self::InvalidReceipt => "harness owner receipt is invalid",
            Self::ValueForNonAccepted => {
                "harness owner returned a value for a non-accepted outcome"
            }
            Self::ResponseTooLarge => "harness owner response exceeds its bound",
            Self::ResponseTooDeep => "harness owner response exceeds its depth bound",
            Self::ForbiddenField => "harness owner response contains a forbidden field",
            Self::OperationMismatch => "harness owner receipt operation does not match the request",
            Self::ReadHadEffects => "harness owner read response reports an effect",
        })
    }
}

impl std::error::Error for OwnerResponseError {}

/// The integration seam owned by the harness team.
///
/// Implementations should journal effects before returning an accepted receipt. On a disconnected
/// response they return [`OwnerError::LostReply`] with the owner's receipt identifier. The target
/// then performs exactly one `lookup_receipt` call and never retries the original `call`.
pub trait HarnessOwner: Send + Sync {
    fn call(&self, request: OwnerCall) -> Result<OwnerReply, OwnerError>;

    fn lookup_receipt(&self, request: OwnerReceiptLookup) -> Result<OwnerReply, OwnerError>;
}

pub(crate) fn owner_scope(
    project_id: &str,
    run_id: &str,
    episode_id: &str,
    agent_id: &str,
) -> OwnerScope {
    OwnerScope {
        project_id: project_id.to_owned(),
        run_id: run_id.to_owned(),
        episode_id: episode_id.to_owned(),
        agent_id: agent_id.to_owned(),
    }
}

pub(crate) fn owner_call(
    operation: OwnerOperation,
    scope: OwnerScope,
    reference: Option<String>,
    payload: Value,
    grant: OwnerGrantReceipt,
) -> Result<OwnerCall, OwnerResponseError> {
    if !scope.is_valid() {
        return Err(OwnerResponseError::InvalidScope);
    }
    validate_public_value(&payload)?;
    let request_sha256 = canonical_digest(&payload);
    Ok(OwnerCall {
        operation,
        scope,
        reference,
        payload,
        request_sha256,
        grant,
    })
}

pub(crate) fn validate_public_value(value: &Value) -> Result<(), OwnerResponseError> {
    let bytes = serde_json::to_vec(value).map_err(|_| OwnerResponseError::ResponseTooLarge)?;
    if bytes.len() > OWNER_MAX_RESPONSE_BYTES {
        return Err(OwnerResponseError::ResponseTooLarge);
    }
    validate_value_inner(value, 0)
}

fn validate_value_inner(value: &Value, depth: usize) -> Result<(), OwnerResponseError> {
    if depth > OWNER_MAX_RESPONSE_DEPTH {
        return Err(OwnerResponseError::ResponseTooDeep);
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if forbidden_field(key, child) {
                    return Err(OwnerResponseError::ForbiddenField);
                }
                validate_value_inner(child, depth.saturating_add(1))?;
            }
        }
        Value::Array(values) => {
            for child in values {
                validate_value_inner(child, depth.saturating_add(1))?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn forbidden_field(key: &str, value: &Value) -> bool {
    let normalized = key.to_ascii_lowercase();
    let authorization_reference =
        normalized.ends_with("_authorization_ref") && value.as_str().is_some_and(valid_id);
    if normalized.contains("credential")
        || normalized.contains("private")
        || normalized.contains("secret")
        || normalized.contains("password")
        || normalized.contains("authorization") && !authorization_reference
        || normalized.contains("bearer")
        || normalized.contains("token")
        || normalized.contains("prompt")
        || normalized.contains("request_bytes")
        || normalized.contains("content_bytes")
    {
        return true;
    }
    let safe_native_metadata = [
        "native_version",
        "native_binary_sha256",
        "native_schema_sha256",
        "native_calls",
    ]
    .iter()
    .any(|allowed| normalized == *allowed);
    if normalized.contains("native") && !safe_native_metadata {
        return true;
    }
    let rpc_capability = normalized == "raw_rpc"
        || normalized == "rpc"
        || normalized.starts_with("rpc_")
        || normalized.ends_with("_rpc");
    rpc_capability && !matches!(value, Value::Bool(false))
}

fn validate_receipt(receipt: &OwnerReceipt) -> Result<(), OwnerResponseError> {
    if receipt.schema != OWNER_RECEIPT_SCHEMA
        || !valid_id(&receipt.receipt_id)
        || !valid_id(&receipt.operation_id)
        || !safe_metadata_id(&receipt.source)
        || !safe_metadata_id(&receipt.evidence)
        || receipt.operation.is_empty()
        || receipt.operation.len() > 128
        || !receipt
            .operation
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(OwnerResponseError::InvalidReceipt);
    }
    if receipt.outcome != OwnerOutcome::Accepted && receipt.effect_applied {
        return Err(OwnerResponseError::InvalidReceipt);
    }
    Ok(())
}

fn safe_metadata_id(value: &str) -> bool {
    valid_id(value)
        && [
            "credential",
            "private",
            "secret",
            "password",
            "authorization",
            "bearer",
            "token",
            "prompt",
        ]
        .iter()
        .all(|needle| !value.to_ascii_lowercase().contains(needle))
}

fn nonzero_effect(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            ["inference_calls", "native_calls", "game_effects"]
                .iter()
                .any(|field| {
                    object
                        .get(*field)
                        .and_then(Value::as_u64)
                        .is_some_and(|count| count > 0)
                })
                || object.values().any(nonzero_effect)
        }
        Value::Array(values) => values.iter().any(nonzero_effect),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

#[allow(clippy::manual_unwrap_or_default)]
fn canonical_digest(value: &Value) -> String {
    let canonical = canonical_value(value);
    let bytes = match serde_json::to_vec(&canonical) {
        Ok(bytes) => bytes,
        Err(_) => Vec::new(),
    };
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut sorted = serde_json::Map::new();
            for (key, child) in entries {
                sorted.insert(key.clone(), canonical_value(child));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        _ => value.clone(),
    }
}
