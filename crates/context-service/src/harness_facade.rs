// SPDX-License-Identifier: MIT

//! Bounded, non-demo composition for the Context Console and the harness owner.
//!
//! The console is deliberately not a scheduler, provider client, game client, or URL proxy.  The
//! [`HarnessOwnerPort`] is the only seam through which control operations can be delegated.  An
//! implementation is expected to resolve the opaque [`ProtectedAuthReference`] inside the owning
//! harness process; the reference itself never contains a credential.  The port is intentionally
//! small and typed so the upstream harness can settle its concrete transport independently.
//!
//! This module is separate from `demo`: the demo still owns its fixture tokens and routes, while
//! this composition requires caller-injected grants, an injected owner-auth reference, exact
//! host/origin proof, and a digest of an injected CSRF secret.  No provider/game credentials,
//! arbitrary upstream URLs, prepared bytes, or scheduler methods appear in this API.

use crate::control::{
    Boundary, Capabilities as ControlCapabilities, Command, ControlError, Draft, EligibleItem,
    ItemRef, Patch, Preview, Receipt, Revision, Scope, State,
};
use crate::http::{HttpRequest, HttpResponse, MAX_HTTP_BODY_BYTES, split_target};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const FACADE_CAPABILITIES_SCHEMA: &str =
    "ascension.context-control.harness-facade-capabilities.v1";
pub const FACADE_ERROR_SCHEMA: &str = "ascension.context-control.harness-facade-error.v1";
pub const FACADE_CONFIG_SCHEMA: &str = "ascension.context-control.harness-facade-config.v1";
pub const MAX_FACADE_ID_BYTES: usize = 128;
pub const MAX_FACADE_HOST_BYTES: usize = 256;
pub const MAX_FACADE_ORIGIN_BYTES: usize = 512;
pub const MAX_FACADE_PRINCIPAL_BYTES: usize = 128;
pub const MAX_FACADE_TOKEN_BYTES: usize = 2048;
pub const MAX_FACADE_BODY_BYTES: usize = 16 * 1024;
/// Maximum serialized JSON response emitted by the facade.
///
/// This is deliberately the same bound as the request body so an owner cannot turn a bounded
/// typed collection into an unbounded transport response.
pub const MAX_FACADE_RESPONSE_BYTES: usize = MAX_FACADE_BODY_BYTES;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_FACADE_LOCKED_REASON_BYTES: usize = 256;
const MAX_FACADE_BLOCKERS: usize = 32;
const MAX_FACADE_COMPONENTS: usize = 128;

const ELIGIBLE_ITEM_KINDS: &[&str] = &[
    "history",
    "note",
    "objective",
    "protected_state",
    // These are the names accepted by the versioned Phase 2 control contract.  The synthetic
    // owner uses the shorter fixture names above.
    "operator_note",
    "historical_artifact",
    "map_artifact",
    "operating_constraint",
];
const LOCKED_REASON_ALLOWLIST: &[&str] = &[
    "host-owned",
    "host-owned state and legal catalog cannot be edited",
    "protected",
    "expired",
    "unavailable",
];
const STATE_STATUSES: &[&str] = &[
    "running",
    "pause_requested",
    "draining_provider",
    "reconciling_action",
    "paused_ready",
    "paused_stale",
    "paused_committed",
    "blocked_recovery",
    "stopped",
];
const RECEIPT_KINDS: &[&str] = &["pause", "commit", "resume"];
const RECEIPT_STATUSES: &[&str] = &[
    "accepted",
    "pending",
    "completed",
    "rejected",
    "blocked",
    "unknown",
];
const RECEIPT_EFFECTS: &[&str] = &[
    "none",
    "pause_latched",
    "pause_requested",
    "paused_ready",
    "revision_committed",
    "no_change",
    "resume_accepted",
    "resume_claimed",
    "scheduler_released",
];

const OWNER_READ_CAPABILITIES: &[&str] = &[
    "capabilities",
    "state",
    "eligible_items",
    "revisions",
    "drafts",
    "previews",
    "receipts",
];
const OWNER_CONTROL_CAPABILITIES: &[&str] = &[
    "create_draft",
    "apply_patch",
    "create_preview",
    "pause",
    "commit",
    "resume",
];

/// Capture/retention mode advertised by the non-demo composition.
///
/// The default is [`RetentionMode::Off`].  Private retention is only accepted when the caller
/// explicitly supplies an accepted policy and authenticated-encryption proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionMode {
    Off,
    Metadata,
    Memory,
    PrivateEncrypted,
}

/// Compatibility alias used by consumers that call this a capture mode.
pub type FacadeCaptureMode = RetentionMode;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionPolicy {
    pub mode: RetentionMode,
    pub policy_accepted: bool,
    pub authenticated_encryption: bool,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            mode: RetentionMode::Off,
            policy_accepted: false,
            authenticated_encryption: false,
        }
    }
}

impl RetentionPolicy {
    pub fn validate(&self) -> Result<(), FacadeConfigError> {
        if self.mode == RetentionMode::PrivateEncrypted
            && (!self.policy_accepted || !self.authenticated_encryption)
        {
            return Err(FacadeConfigError::PrivateRetentionNotApproved);
        }
        if self.mode != RetentionMode::PrivateEncrypted
            && self.authenticated_encryption
            && !self.policy_accepted
        {
            return Err(FacadeConfigError::InvalidRetentionPolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FacadeConfigError {
    InvalidAuthReference,
    InvalidScope,
    InvalidSchema,
    InvalidHost,
    InvalidOrigin,
    MissingCsrfProof,
    PrivateRetentionNotApproved,
    InvalidRetentionPolicy,
}

impl std::fmt::Display for FacadeConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidAuthReference => "protected auth reference is invalid",
            Self::InvalidScope => "facade scope is invalid",
            Self::InvalidSchema => "facade configuration schema is invalid",
            Self::InvalidHost => "expected host is invalid",
            Self::InvalidOrigin => "expected origin is invalid",
            Self::MissingCsrfProof => "a CSRF proof digest is required for writes",
            Self::PrivateRetentionNotApproved => {
                "private retention requires accepted policy and authenticated encryption"
            }
            Self::InvalidRetentionPolicy => "retention policy is invalid",
        })
    }
}

impl std::error::Error for FacadeConfigError {}

/// An opaque reference resolved by the harness owner.  It is not a secret and cannot contain a
/// URL, path, or credential.  The owner may map it to a vault key, capability broker entry, or
/// another protected local mechanism.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtectedAuthReference(String);

impl ProtectedAuthReference {
    pub fn new(value: impl Into<String>) -> Result<Self, FacadeConfigError> {
        let value = value.into();
        if valid_reference(&value) {
            Ok(Self(value))
        } else {
            Err(FacadeConfigError::InvalidAuthReference)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A one-way digest of an injected bearer/CSRF secret.  Raw secret bytes are never retained by
/// the facade configuration or its debug/serialization projections.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretDigest([u8; 32]);

impl SecretDigest {
    pub fn from_secret(secret: &[u8]) -> Result<Self, FacadeConfigError> {
        if secret.is_empty() {
            return Err(FacadeConfigError::MissingCsrfProof);
        }
        Ok(Self(Sha256::digest(secret).into()))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn matches(&self, secret: &[u8]) -> bool {
        let candidate: [u8; 32] = Sha256::digest(secret).into();
        constant_time_eq(&self.0, &candidate)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Non-secret, deployment-specific listener policy for a non-demo facade.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessFacadeConfig {
    pub schema: String,
    pub scope: Scope,
    pub expected_host: String,
    pub expected_origin: Option<String>,
    pub csrf_digest: Option<SecretDigest>,
    pub retention: RetentionPolicy,
}

impl HarnessFacadeConfig {
    pub fn new(
        scope: Scope,
        expected_host: impl Into<String>,
        expected_origin: Option<String>,
        csrf_digest: Option<SecretDigest>,
        retention: RetentionPolicy,
    ) -> Result<Self, FacadeConfigError> {
        let config = Self {
            schema: FACADE_CONFIG_SCHEMA.to_owned(),
            scope,
            expected_host: expected_host.into(),
            expected_origin,
            csrf_digest,
            retention,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), FacadeConfigError> {
        if self.schema != FACADE_CONFIG_SCHEMA {
            return Err(FacadeConfigError::InvalidSchema);
        }
        if !scope_valid(&self.scope) {
            return Err(FacadeConfigError::InvalidScope);
        }
        if !valid_host(&self.expected_host) {
            return Err(FacadeConfigError::InvalidHost);
        }
        if self
            .expected_origin
            .as_deref()
            .is_some_and(|origin| !valid_origin(origin))
        {
            return Err(FacadeConfigError::InvalidOrigin);
        }
        self.retention.validate()?;
        Ok(())
    }

    pub fn writes_configured(&self) -> bool {
        self.csrf_digest.is_some()
    }
}

/// An authenticated request envelope.  The bearer and CSRF bytes are intentionally private and
/// the `Debug` implementation redacts them; consumers should construct this from their protected
/// request middleware rather than serialize it or put it in a URL.
#[derive(Clone, Eq, PartialEq)]
pub struct FacadeRequest {
    token: Vec<u8>,
    principal: String,
    host: String,
    origin: Option<String>,
    csrf_token: Option<Vec<u8>>,
    now: u64,
}

impl std::fmt::Debug for FacadeRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FacadeRequest")
            .field("token", &"<redacted>")
            .field("principal", &self.principal)
            .field("host", &self.host)
            .field("origin", &self.origin)
            .field(
                "csrf_token",
                &self.csrf_token.as_ref().map(|_| "<redacted>"),
            )
            .field("now", &self.now)
            .finish()
    }
}

impl FacadeRequest {
    pub fn new(
        principal: impl Into<String>,
        token: impl AsRef<[u8]>,
        host: impl Into<String>,
        origin: Option<String>,
        csrf_token: Option<&[u8]>,
        now: u64,
    ) -> Self {
        Self {
            token: token.as_ref().to_vec(),
            principal: principal.into(),
            host: host.into(),
            origin,
            csrf_token: csrf_token.map(|value| value.as_ref().to_vec()),
            now,
        }
    }

    pub fn new_owned(
        principal: impl Into<String>,
        token: Vec<u8>,
        host: impl Into<String>,
        origin: Option<String>,
        csrf_token: Option<Vec<u8>>,
        now: u64,
    ) -> Self {
        Self {
            token,
            principal: principal.into(),
            host: host.into(),
            origin,
            csrf_token,
            now,
        }
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn origin(&self) -> Option<&str> {
        self.origin.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacadePermission {
    MetadataRead,
    ContentRead,
    Edit,
    Objective,
    Commit,
    Pause,
    Resume,
}

impl FacadePermission {
    const ALL: &'static [&'static str] = &[
        "context.metadata.read",
        "context.content.read",
        "context.edit",
        "context.objective.edit",
        "context.commit",
        "context.pause",
        "context.resume",
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MetadataRead => "context.metadata.read",
            Self::ContentRead => "context.content.read",
            Self::Edit => "context.edit",
            Self::Objective => "context.objective.edit",
            Self::Commit => "context.commit",
            Self::Pause => "context.pause",
            Self::Resume => "context.resume",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GrantError {
    InvalidGrantId,
    EmptyToken,
    InvalidExpiry,
    DuplicateGrant,
    NotFound,
    Revoked,
}

impl std::fmt::Display for GrantError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidGrantId => "grant identity is invalid",
            Self::EmptyToken => "grant token is empty",
            Self::InvalidExpiry => "grant expiry is invalid",
            Self::DuplicateGrant => "grant identity is already in use",
            Self::NotFound => "grant is unavailable",
            Self::Revoked => "grant has been revoked",
        })
    }
}

impl std::error::Error for GrantError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityGrant {
    pub grant_id: String,
    pub permission: FacadePermission,
    pub scope: Scope,
    pub token_digest: SecretDigest,
    pub expires_at: u64,
    pub revoked: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GrantRegistry {
    grants: BTreeMap<String, CapabilityGrant>,
}

impl GrantRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(
        &mut self,
        grant_id: impl Into<String>,
        permission: FacadePermission,
        scope: Scope,
        token: &[u8],
        expires_at: u64,
    ) -> Result<CapabilityGrant, GrantError> {
        let grant_id = grant_id.into();
        if !valid_id(&grant_id) {
            return Err(GrantError::InvalidGrantId);
        }
        if token.is_empty() {
            return Err(GrantError::EmptyToken);
        }
        if expires_at == 0 {
            return Err(GrantError::InvalidExpiry);
        }
        if self.grants.contains_key(&grant_id) {
            return Err(GrantError::DuplicateGrant);
        }
        let grant = CapabilityGrant {
            grant_id: grant_id.clone(),
            permission,
            scope,
            token_digest: SecretDigest(Sha256::digest(token).into()),
            expires_at,
            revoked: false,
        };
        self.grants.insert(grant_id, grant.clone());
        Ok(grant)
    }

    pub fn insert(&mut self, grant: CapabilityGrant) -> Result<(), GrantError> {
        if !valid_id(&grant.grant_id) || grant.expires_at == 0 {
            return Err(GrantError::InvalidGrantId);
        }
        if self.grants.contains_key(&grant.grant_id) {
            return Err(GrantError::DuplicateGrant);
        }
        self.grants.insert(grant.grant_id.clone(), grant);
        Ok(())
    }

    pub fn revoke(&mut self, grant_id: &str) -> Result<(), GrantError> {
        let grant = self.grants.get_mut(grant_id).ok_or(GrantError::NotFound)?;
        grant.revoked = true;
        Ok(())
    }

    pub fn unrevoke(&mut self, grant_id: &str) -> Result<(), GrantError> {
        let grant = self.grants.get_mut(grant_id).ok_or(GrantError::NotFound)?;
        grant.revoked = false;
        Ok(())
    }

    pub fn get(&self, grant_id: &str) -> Option<&CapabilityGrant> {
        self.grants.get(grant_id)
    }

    pub fn authorize(
        &self,
        token: &[u8],
        permission: FacadePermission,
        scope: &Scope,
        now: u64,
    ) -> Result<(), GrantError> {
        if token.is_empty() {
            return Err(GrantError::EmptyToken);
        }
        let digest: [u8; 32] = Sha256::digest(token).into();
        let mut revoked = false;
        let mut expired = false;
        for grant in self.grants.values().filter(|grant| {
            grant.permission == permission
                && grant.scope == *scope
                && constant_time_eq(grant.token_digest.as_bytes(), &digest)
        }) {
            if grant.revoked {
                revoked = true;
            } else if now >= grant.expires_at {
                expired = true;
            } else {
                return Ok(());
            }
        }
        if revoked {
            Err(GrantError::Revoked)
        } else if expired {
            Err(GrantError::InvalidExpiry)
        } else {
            Err(GrantError::NotFound)
        }
    }

    pub fn permissions(&self) -> Vec<&'static str> {
        self.permissions_filtered(None)
    }

    pub fn permissions_at(&self, now: u64) -> Vec<&'static str> {
        self.permissions_filtered(Some(now))
    }

    pub fn permissions_for(&self, scope: &Scope, now: u64) -> Vec<&'static str> {
        let mut permissions = self
            .grants
            .values()
            .filter(|grant| grant.scope == *scope && !grant.revoked && now < grant.expires_at)
            .map(|grant| grant.permission.as_str())
            .collect::<Vec<_>>();
        permissions.sort_unstable();
        permissions.dedup();
        permissions
    }

    fn permissions_filtered(&self, now: Option<u64>) -> Vec<&'static str> {
        let mut permissions = self
            .grants
            .values()
            .filter(|grant| !grant.revoked && now.is_none_or(|current| current < grant.expires_at))
            .map(|grant| grant.permission.as_str())
            .collect::<Vec<_>>();
        permissions.sort_unstable();
        permissions.dedup();
        permissions
    }
}

/// Authorization asserted by the console after independently checking grants.  The harness must
/// still treat this as an assertion to validate against its own owner policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerAuthorization {
    pub objective_override: bool,
}

impl OwnerAuthorization {
    pub const fn ordinary() -> Self {
        Self {
            objective_override: false,
        }
    }

    pub const fn with_objective() -> Self {
        Self {
            objective_override: true,
        }
    }
}

/// Stable owner outcomes.  Messages and upstream error bodies are intentionally not carried over
/// the console boundary, so private content and provider details cannot leak into logs or URLs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnerError {
    Unsupported,
    Unavailable,
    Denied,
    Stale,
    Unknown,
    NotFound,
    Expired,
    Invalid,
}

impl OwnerError {
    fn from_control(error: ControlError) -> Self {
        let code = error.code;
        if code.contains("stale")
            || code.contains("conflict")
            || code.contains("already")
            || code.contains("unresolved")
            || code.contains("preview")
        {
            Self::Stale
        } else if code.contains("disabled")
            || code.contains("forbidden")
            || code.contains("authorization")
            || code.contains("protected")
            || code.contains("stopped")
        {
            Self::Denied
        } else if code.contains("expired") {
            Self::Expired
        } else if code.contains("not_found") || code.contains("unknown") {
            Self::NotFound
        } else {
            Self::Invalid
        }
    }
}

/// Typed owner port.  It contains no scheduler, provider, game, URL, or credential operation.
///
/// `list_*` methods are used only to build a local, non-secret reference index.  A real harness
/// implementation may return `OwnerError::Unsupported`; in that case the facade fails closed
/// rather than forwarding an unverified foreign reference.
pub trait HarnessOwnerPort {
    fn capabilities(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<ControlCapabilities, OwnerError>;
    fn state(&mut self, auth: &ProtectedAuthReference, scope: &Scope) -> Result<State, OwnerError>;
    fn eligible_items(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<EligibleItem>, OwnerError>;
    fn revisions(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Revision>, OwnerError>;
    fn drafts(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Draft>, OwnerError>;
    fn previews(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Preview>, OwnerError>;
    fn receipts(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Receipt>, OwnerError>;
    fn get_draft(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
        draft_id: &str,
    ) -> Result<Draft, OwnerError>;
    fn get_preview(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
        preview_id: &str,
    ) -> Result<Preview, OwnerError>;
    fn get_receipt(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: &Scope,
        command_id: &str,
    ) -> Result<Receipt, OwnerError>;
    fn create_draft(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: Scope,
        expected_active_revision_id: &str,
        author_ref: &str,
    ) -> Result<Draft, OwnerError>;
    #[allow(clippy::too_many_arguments)]
    fn apply_patch(
        &mut self,
        auth: &ProtectedAuthReference,
        patch: Patch,
        author_ref: &str,
        authorization: OwnerAuthorization,
    ) -> Result<Draft, OwnerError>;
    #[allow(clippy::too_many_arguments)]
    fn create_preview(
        &mut self,
        auth: &ProtectedAuthReference,
        scope: Scope,
        draft_id: &str,
        expected_draft_version: u64,
        applicable_requested: bool,
        expected_control_version: u64,
        risk_ack: bool,
    ) -> Result<Preview, OwnerError>;
    fn pause(
        &mut self,
        auth: &ProtectedAuthReference,
        command: Command,
    ) -> Result<Receipt, OwnerError>;
    fn commit(
        &mut self,
        auth: &ProtectedAuthReference,
        command: Command,
    ) -> Result<Receipt, OwnerError>;
    fn resume(
        &mut self,
        auth: &ProtectedAuthReference,
        command: Command,
    ) -> Result<Receipt, OwnerError>;
}

/// A typed client carrying only the injected owner-auth reference.
#[derive(Clone, Debug)]
pub struct HarnessOwnerClient<O> {
    owner: O,
    auth_reference: ProtectedAuthReference,
}

impl<O> HarnessOwnerClient<O> {
    pub fn new(owner: O, auth_reference: ProtectedAuthReference) -> Self {
        Self {
            owner,
            auth_reference,
        }
    }

    pub fn owner(&self) -> &O {
        &self.owner
    }

    pub fn into_owner(self) -> O {
        self.owner
    }

    pub fn auth_reference(&self) -> &ProtectedAuthReference {
        &self.auth_reference
    }

    fn call<R>(&mut self, operation: impl FnOnce(&mut O, &ProtectedAuthReference) -> R) -> R {
        // Clone only the opaque identifier so the owner and auth borrows stay disjoint.  This
        // value never contains a credential.
        let auth = self.auth_reference.clone();
        operation(&mut self.owner, &auth)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FacadeErrorClass {
    Invalid,
    Unauthorized,
    Denied,
    Unavailable,
    Stale,
    Unknown,
    NotFound,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacadeError {
    pub schema: String,
    pub code: String,
    pub class: FacadeErrorClass,
    pub retryable: bool,
}

pub type FacadeResult<T> = Result<T, FacadeError>;

impl std::fmt::Display for FacadeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({:?})", self.code, self.class)
    }
}

impl std::error::Error for FacadeError {}

impl FacadeError {
    fn new(code: &'static str, class: FacadeErrorClass, retryable: bool) -> Self {
        Self {
            schema: FACADE_ERROR_SCHEMA.to_owned(),
            code: code.to_owned(),
            class,
            retryable,
        }
    }

    fn invalid(code: &'static str) -> Self {
        Self::new(code, FacadeErrorClass::Invalid, false)
    }

    fn unauthorized(code: &'static str) -> Self {
        Self::new(code, FacadeErrorClass::Unauthorized, false)
    }

    fn denied(code: &'static str) -> Self {
        Self::new(code, FacadeErrorClass::Denied, false)
    }

    fn stale(code: &'static str) -> Self {
        Self::new(code, FacadeErrorClass::Stale, true)
    }

    fn expired(code: &'static str) -> Self {
        Self::new(code, FacadeErrorClass::Expired, false)
    }

    fn response_too_large() -> Self {
        Self::new("response_too_large", FacadeErrorClass::Unavailable, false)
    }

    fn owner(error: OwnerError) -> Self {
        match error {
            OwnerError::Unsupported => Self::new(
                "owner_capability_unavailable",
                FacadeErrorClass::Unavailable,
                false,
            ),
            OwnerError::Unavailable => {
                Self::new("owner_unavailable", FacadeErrorClass::Unavailable, true)
            }
            OwnerError::Denied => Self::new("owner_denied", FacadeErrorClass::Denied, false),
            OwnerError::Stale => Self::new("owner_stale", FacadeErrorClass::Stale, true),
            OwnerError::Unknown => Self::new("owner_unknown", FacadeErrorClass::Unknown, true),
            OwnerError::NotFound => Self::new("owner_not_found", FacadeErrorClass::NotFound, false),
            OwnerError::Expired => Self::new("owner_expired", FacadeErrorClass::Expired, false),
            OwnerError::Invalid => Self::new("owner_invalid", FacadeErrorClass::Invalid, false),
        }
    }

    fn grant(permission: FacadePermission, error: GrantError) -> Self {
        match error {
            GrantError::InvalidExpiry => Self::expired("grant_expired"),
            GrantError::Revoked => Self::denied("grant_revoked"),
            GrantError::EmptyToken | GrantError::NotFound => Self::unauthorized(match permission {
                FacadePermission::MetadataRead => "metadata_grant_required",
                FacadePermission::ContentRead => "content_grant_required",
                FacadePermission::Edit => "edit_grant_required",
                FacadePermission::Objective => "objective_grant_required",
                FacadePermission::Commit => "commit_grant_required",
                FacadePermission::Pause => "pause_grant_required",
                FacadePermission::Resume => "resume_grant_required",
            }),
            GrantError::InvalidGrantId | GrantError::DuplicateGrant => {
                Self::invalid("grant_invalid")
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacadeCapabilities {
    pub schema: String,
    pub composition: String,
    pub scope: Scope,
    pub owner_enabled: bool,
    pub owner_supported_operations: Vec<String>,
    pub forwarded_operations: Vec<String>,
    pub grant_permissions: Vec<String>,
    pub retention_mode: RetentionMode,
    pub capture_default_off: bool,
    pub exact_application_preview: String,
    pub provider_added_context: String,
    pub direct_game_dispatch: bool,
    pub duplicate_scheduler: bool,
    pub legacy_demo: bool,
}

#[derive(Clone, Debug, Default)]
struct ReferenceIndex {
    item_refs: BTreeMap<(String, u64), ItemRef>,
    revision_ids: BTreeSet<String>,
    revisions: BTreeMap<String, Revision>,
    draft_ids: BTreeSet<String>,
    drafts: BTreeMap<String, Draft>,
    preview_ids: BTreeSet<String>,
    receipt_ids: BTreeSet<String>,
    items_complete: bool,
    revisions_complete: bool,
    drafts_complete: bool,
    previews_complete: bool,
    receipts_complete: bool,
}

/// Non-demo, harness-backed Context Console composition.
pub struct HarnessBackedContextService<O> {
    client: HarnessOwnerClient<O>,
    grants: GrantRegistry,
    config: HarnessFacadeConfig,
    owner_capabilities: Option<ControlCapabilities>,
    owner_capability_error: Option<OwnerError>,
    index: ReferenceIndex,
}

/// Request shape for a deterministic exploratory or held-boundary preview.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewRequest {
    pub scope: Scope,
    pub draft_id: String,
    pub expected_draft_version: u64,
    pub applicable_requested: bool,
    pub expected_control_version: u64,
    pub unknown_total_risk_acknowledged: bool,
}

impl<O: HarnessOwnerPort> HarnessBackedContextService<O> {
    pub fn new(
        client: HarnessOwnerClient<O>,
        grants: GrantRegistry,
        config: HarnessFacadeConfig,
    ) -> FacadeResult<Self> {
        config
            .validate()
            .map_err(|_| FacadeError::invalid("facade_config_invalid"))?;
        let mut service = Self {
            client,
            grants,
            config,
            owner_capabilities: None,
            owner_capability_error: None,
            index: ReferenceIndex::default(),
        };
        service.load_owner_capabilities();
        service.refresh_reference_index();
        Ok(service)
    }

    pub fn config(&self) -> &HarnessFacadeConfig {
        &self.config
    }

    pub fn grants(&self) -> &GrantRegistry {
        &self.grants
    }

    pub fn grants_mut(&mut self) -> &mut GrantRegistry {
        &mut self.grants
    }

    pub fn owner(&self) -> &O {
        self.client.owner()
    }

    pub fn into_owner(self) -> O {
        self.client.into_owner()
    }

    /// Handle the versioned HTTP transport used by Studio.  The transport is only an adapter:
    /// validation, authorization, and delegation remain in the typed methods below.
    pub fn handle_http(&mut self, request: &HttpRequest) -> HttpResponse {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.handle_http_at(request, now)
    }

    /// Deterministic HTTP entry point used by process tests and replay tooling.
    pub fn handle_http_at(&mut self, request: &HttpRequest, now: u64) -> HttpResponse {
        if duplicate_security_header(request, "host")
            || duplicate_security_header(request, "origin")
            || duplicate_security_header(request, "authorization")
            || duplicate_security_header(request, "x-csrf-token")
            || duplicate_security_header(request, "x-principal")
        {
            return facade_http_error(400, "duplicate_security_header");
        }
        if request.target.len() > 4096
            || request.target.contains("://")
            || request.target.contains('%')
            || request.target.contains('\\')
            || request.target.contains('\0')
        {
            return facade_http_error(400, "invalid_target");
        }
        if request.body.len() > MAX_FACADE_BODY_BYTES {
            return facade_http_error(413, "body_too_large");
        }
        if request.method == "GET" && !request.body.is_empty() {
            return facade_http_error(400, "get_body_not_allowed");
        }
        if request.method != "GET" && request.method != "POST" {
            return facade_http_error(405, "method_not_allowed");
        }
        if request.header("host") != Some(self.config.expected_host.as_str()) {
            return facade_http_error(403, "host_not_allowed");
        }
        if self.config.expected_origin.as_deref() != request.header("origin") {
            return facade_http_error(403, "origin_not_allowed");
        }
        if request.method != "GET" {
            let csrf_ok = self
                .config
                .csrf_digest
                .zip(request.header("x-csrf-token"))
                .is_some_and(|(digest, token)| digest.matches(token.as_bytes()));
            if !csrf_ok {
                return facade_http_error(403, "csrf_rejected");
            }
        }
        let (path, query) = split_target(&request.target);
        if !path.starts_with('/')
            || path.len() > 4096
            || path.contains("//")
            || path.ends_with('/')
            || path
                .split('/')
                .skip(1)
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return facade_http_error(400, "invalid_target");
        }
        if query.len() > 8
            || query.iter().any(|(key, value)| {
                key.is_empty()
                    || key.len() > 64
                    || value.len() > 256
                    || key
                        .bytes()
                        .any(|byte| !byte.is_ascii_alphanumeric() && byte != b'_')
            })
            || query
                .iter()
                .enumerate()
                .any(|(index, (key, _))| query[index + 1..].iter().any(|(other, _)| other == key))
        {
            return facade_http_error(400, "invalid_query");
        }
        if query.iter().any(|(key, _)| {
            key.eq_ignore_ascii_case("token")
                || key.eq_ignore_ascii_case("authorization")
                || key.eq_ignore_ascii_case("csrf")
        }) {
            return facade_http_error(400, "secret_must_not_be_in_url");
        }
        let segments = path.split('/').skip(1).collect::<Vec<_>>();
        if segments.len() < 4
            || segments[0] != "v2"
            || segments[1] != "runs"
            || segments[3] != "context-control"
        {
            return facade_http_error(404, "route_not_found");
        }
        if segments[2] != self.config.scope.run_id {
            return facade_http_error(403, "foreign_reference");
        }
        let token = request
            .header("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::as_bytes)
            .unwrap_or_default();
        let principal = request.header("x-principal").unwrap_or_default();
        let facade_request = FacadeRequest::new(
            principal,
            token,
            request.header("host").unwrap_or_default(),
            request.header("origin").map(str::to_owned),
            request.header("x-csrf-token").map(str::as_bytes),
            now,
        );
        let tail = &segments[4..];
        let result = match (request.method.as_str(), tail) {
            ("GET", ["capabilities"]) => self.capabilities(&facade_request).and_then(to_value),
            ("GET", ["state"]) => self.state(&facade_request).and_then(to_value),
            ("GET", ["eligible-items"]) => {
                let include_content = match query.as_slice() {
                    [] => false,
                    [(key, value)] if key == "include_content" && value == "true" => true,
                    [(key, value)] if key == "include_content" && value == "false" => false,
                    _ => {
                        return facade_http_result::<serde_json::Value>(Err(FacadeError::invalid(
                            "invalid_query",
                        )));
                    }
                };
                self.eligible_items(&facade_request, include_content)
                    .and_then(to_value)
            }
            ("GET", _) if !query.is_empty() => Err(FacadeError::invalid("invalid_query")),
            ("GET", ["revisions"]) => self.revisions(&facade_request).and_then(to_value),
            ("GET", ["drafts"]) => self.drafts(&facade_request).and_then(to_value),
            ("GET", ["drafts", draft_id]) => self
                .read_draft(&facade_request, draft_id)
                .and_then(to_value),
            ("GET", ["previews"]) => self.previews(&facade_request).and_then(to_value),
            ("GET", ["previews", preview_id]) => self
                .read_preview(&facade_request, preview_id)
                .and_then(to_value),
            ("GET", ["commands", command_id]) => self
                .read_receipt(&facade_request, command_id)
                .and_then(to_value),
            ("POST", ["drafts"]) => self
                .http_create_draft(&facade_request, &request.body)
                .and_then(to_value),
            ("POST", ["drafts", draft_id, "operations"]) => self
                .http_edit_draft(&facade_request, draft_id, &request.body)
                .and_then(to_value),
            ("POST", ["previews"]) => self
                .http_preview(&facade_request, &request.body)
                .and_then(to_value),
            ("POST", ["pause"]) => self
                .http_command(&facade_request, "pause", &request.body)
                .and_then(to_value),
            ("POST", ["commits"]) => self
                .http_command(&facade_request, "commit", &request.body)
                .and_then(to_value),
            ("POST", ["resume"]) => self
                .http_command(&facade_request, "resume", &request.body)
                .and_then(to_value),
            _ => Err(FacadeError::invalid("route_not_found")),
        };
        facade_http_result(result)
    }

    fn http_create_draft(&mut self, request: &FacadeRequest, body: &[u8]) -> FacadeResult<Draft> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Body {
            expected_active_revision_id: String,
        }
        let body: Body = parse_facade_json(body)?;
        self.create_draft(request, &body.expected_active_revision_id)
    }

    fn http_edit_draft(
        &mut self,
        request: &FacadeRequest,
        draft_id: &str,
        body: &[u8],
    ) -> FacadeResult<Draft> {
        let patch: Patch = parse_facade_json(body)?;
        if patch.draft_id != draft_id {
            return Err(FacadeError::invalid("draft_mismatch"));
        }
        self.edit_draft(request, patch)
    }

    fn http_preview(&mut self, request: &FacadeRequest, body: &[u8]) -> FacadeResult<Preview> {
        let preview: PreviewRequest = parse_facade_json(body)?;
        self.preview(request, preview)
    }

    fn http_command(
        &mut self,
        request: &FacadeRequest,
        kind: &str,
        body: &[u8],
    ) -> FacadeResult<Receipt> {
        let command: Command = parse_facade_json(body)?;
        match kind {
            "pause" => self.pause(request, command),
            "commit" => self.held_commit(request, command),
            "resume" => self.resume(request, command),
            _ => Err(FacadeError::invalid("invalid_command")),
        }
    }

    /// Refresh owner capabilities after an external owner policy change.
    pub fn refresh_capabilities(&mut self, request: &FacadeRequest) -> FacadeResult<()> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.load_owner_capabilities();
        self.refresh_reference_index();
        self.owner_capability_error
            .map_or(Ok(()), |error| Err(FacadeError::owner(error)))
    }

    pub fn capabilities(&mut self, request: &FacadeRequest) -> FacadeResult<FacadeCapabilities> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        let capabilities = self.owner_capabilities.clone().ok_or_else(|| {
            FacadeError::owner(self.owner_capability_error.unwrap_or(OwnerError::Unknown))
        })?;
        let mut owner_supported = capabilities
            .supported_operations
            .iter()
            .filter(|operation| known_owner_operation(operation))
            .cloned()
            .collect::<Vec<_>>();
        owner_supported.sort();
        owner_supported.dedup();
        let mut forwarded = if capabilities.enabled {
            OWNER_READ_CAPABILITIES
                .iter()
                .chain(OWNER_CONTROL_CAPABILITIES)
                .filter(|operation| owner_supports_list(&owner_supported, operation))
                .map(|operation| (*operation).to_owned())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        forwarded.sort();
        forwarded.dedup();
        let result = FacadeCapabilities {
            schema: FACADE_CAPABILITIES_SCHEMA.to_owned(),
            composition: "harness_backed".to_owned(),
            scope: self.config.scope.clone(),
            owner_enabled: capabilities.enabled,
            owner_supported_operations: owner_supported,
            forwarded_operations: forwarded,
            grant_permissions: self
                .grants
                .permissions_for(&self.config.scope, request.now())
                .into_iter()
                .map(str::to_owned)
                .collect(),
            retention_mode: self.config.retention.mode,
            capture_default_off: self.config.retention.mode == RetentionMode::Off,
            exact_application_preview: if capabilities.enabled
                && owner_supports_list(&capabilities.supported_operations, "create_preview")
                && capabilities.exact_application_preview == "supported"
            {
                "owner_conditional".to_owned()
            } else {
                "unavailable".to_owned()
            },
            // The owner may know this value, but the console cannot prove or reconstruct
            // provider-added context and therefore keeps the result explicitly unknown.
            provider_added_context: "unknown".to_owned(),
            direct_game_dispatch: false,
            duplicate_scheduler: false,
            legacy_demo: false,
        };
        validate_facade_capabilities(&result)?;
        Ok(result)
    }

    pub fn state(&mut self, request: &FacadeRequest) -> FacadeResult<State> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.require_owner("state")?;
        let scope = self.config.scope.clone();
        self.client
            .call(|owner, auth| owner.state(auth, &scope))
            .map_err(FacadeError::owner)
            .and_then(|state| self.scoped_state(state))
    }

    pub fn eligible_items(
        &mut self,
        request: &FacadeRequest,
        include_content: bool,
    ) -> FacadeResult<Vec<EligibleItem>> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        if include_content {
            self.authorize(request, FacadePermission::ContentRead, false)?;
        }
        self.require_owner("eligible_items")?;
        let scope = self.config.scope.clone();
        let items = self
            .client
            .call(|owner, auth| owner.eligible_items(auth, &scope))
            .map_err(FacadeError::owner)?;
        let items = match self.project_items(items, include_content) {
            Ok(items) => items,
            Err(error) => {
                self.index.item_refs.clear();
                self.index.items_complete = false;
                return Err(error);
            }
        };
        self.index.item_refs.clear();
        for item in &items {
            self.index.item_refs.insert(
                (item.item.item_id.clone(), item.item.version),
                item.item.clone(),
            );
        }
        self.index.items_complete = true;
        Ok(items)
    }

    pub fn revisions(&mut self, request: &FacadeRequest) -> FacadeResult<Vec<Revision>> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.require_owner("revisions")?;
        let scope = self.config.scope.clone();
        let revisions = self
            .client
            .call(|owner, auth| owner.revisions(auth, &scope))
            .map_err(FacadeError::owner)?;
        let revisions = match self.project_revisions(revisions) {
            Ok(revisions) => revisions,
            Err(error) => {
                self.index.revision_ids.clear();
                self.index.revisions.clear();
                self.index.revisions_complete = false;
                return Err(error);
            }
        };
        self.index.revisions = revisions
            .iter()
            .map(|revision| (revision.revision_id.clone(), revision.clone()))
            .collect();
        self.index.revision_ids = revisions
            .iter()
            .map(|revision| revision.revision_id.clone())
            .collect();
        self.index.revisions_complete = true;
        Ok(revisions)
    }

    pub fn drafts(&mut self, request: &FacadeRequest) -> FacadeResult<Vec<Draft>> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.require_owner("drafts")?;
        let scope = self.config.scope.clone();
        let drafts = self
            .client
            .call(|owner, auth| owner.drafts(auth, &scope))
            .map_err(FacadeError::owner)?;
        let drafts = match self.project_drafts(drafts) {
            Ok(drafts) => drafts,
            Err(error) => {
                self.index.draft_ids.clear();
                self.index.drafts.clear();
                self.index.drafts_complete = false;
                return Err(error);
            }
        };
        self.index.drafts.clear();
        self.index.draft_ids = drafts
            .iter()
            .map(|draft| {
                self.index
                    .drafts
                    .insert(draft.draft_id.clone(), draft.clone());
                draft.draft_id.clone()
            })
            .collect();
        self.index.drafts_complete = true;
        Ok(drafts)
    }

    pub fn read_draft(&mut self, request: &FacadeRequest, draft_id: &str) -> FacadeResult<Draft> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.ensure_known_id(draft_id, &self.index.draft_ids, self.index.drafts_complete)?;
        self.require_owner("drafts")?;
        let scope = self.config.scope.clone();
        let draft_id = draft_id.to_owned();
        let draft = self
            .client
            .call(|owner, auth| owner.get_draft(auth, &scope, &draft_id))
            .map_err(FacadeError::owner)
            .and_then(|draft| self.project_draft(draft))?;
        self.index.draft_ids.insert(draft.draft_id.clone());
        self.index
            .drafts
            .insert(draft.draft_id.clone(), draft.clone());
        Ok(draft)
    }

    pub fn previews(&mut self, request: &FacadeRequest) -> FacadeResult<Vec<Preview>> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.require_owner("previews")?;
        let scope = self.config.scope.clone();
        let previews = self
            .client
            .call(|owner, auth| owner.previews(auth, &scope))
            .map_err(FacadeError::owner)?;
        let previews = match self.project_previews(previews) {
            Ok(previews) => previews,
            Err(error) => {
                self.index.preview_ids.clear();
                self.index.previews_complete = false;
                return Err(error);
            }
        };
        self.index.preview_ids = previews
            .iter()
            .map(|preview| preview.preview_id.clone())
            .collect();
        self.index.previews_complete = true;
        Ok(previews)
    }

    pub fn read_preview(
        &mut self,
        request: &FacadeRequest,
        preview_id: &str,
    ) -> FacadeResult<Preview> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.ensure_known_id(
            preview_id,
            &self.index.preview_ids,
            self.index.previews_complete,
        )?;
        self.require_owner("previews")?;
        let scope = self.config.scope.clone();
        let preview_id = preview_id.to_owned();
        let preview = self
            .client
            .call(|owner, auth| owner.get_preview(auth, &scope, &preview_id))
            .map_err(FacadeError::owner)
            .and_then(|preview| self.project_preview(preview))?;
        self.index.preview_ids.insert(preview.preview_id.clone());
        Ok(preview)
    }

    pub fn read_receipt(
        &mut self,
        request: &FacadeRequest,
        command_id: &str,
    ) -> FacadeResult<Receipt> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.ensure_known_id(
            command_id,
            &self.index.receipt_ids,
            self.index.receipts_complete,
        )?;
        self.require_owner("receipts")?;
        let scope = self.config.scope.clone();
        let command_id = command_id.to_owned();
        let receipt = self
            .client
            .call(|owner, auth| owner.get_receipt(auth, &scope, &command_id))
            .map_err(FacadeError::owner)
            .and_then(|receipt| self.project_receipt(receipt))?;
        self.index.receipt_ids.insert(receipt.command_id.clone());
        Ok(receipt)
    }

    pub fn receipts(&mut self, request: &FacadeRequest) -> FacadeResult<Vec<Receipt>> {
        self.authorize(request, FacadePermission::MetadataRead, false)?;
        self.require_owner("receipts")?;
        let receipts = self
            .client
            .call(|owner, auth| owner.receipts(auth, &self.config.scope.clone()))
            .map_err(FacadeError::owner)
            .and_then(|receipts| match self.project_receipts(receipts) {
                Ok(receipts) => Ok(receipts),
                Err(error) => {
                    self.index.receipt_ids.clear();
                    self.index.receipts_complete = false;
                    Err(error)
                }
            })?;
        self.index.receipt_ids = receipts
            .iter()
            .map(|receipt| receipt.command_id.clone())
            .collect();
        self.index.receipts_complete = true;
        Ok(receipts)
    }

    pub fn create_draft(
        &mut self,
        request: &FacadeRequest,
        expected_active_revision_id: &str,
    ) -> FacadeResult<Draft> {
        self.authorize(request, FacadePermission::Edit, true)?;
        self.ensure_known_id(
            expected_active_revision_id,
            &self.index.revision_ids,
            self.index.revisions_complete,
        )?;
        self.require_owner("create_draft")?;
        let scope = self.config.scope.clone();
        let expected_active_revision_id = expected_active_revision_id.to_owned();
        let author_ref = request.principal().to_owned();
        let draft = self
            .client
            .call(|owner, auth| {
                owner.create_draft(auth, scope, &expected_active_revision_id, &author_ref)
            })
            .map_err(FacadeError::owner)
            .and_then(|draft| self.project_draft(draft))?;
        self.index.draft_ids.insert(draft.draft_id.clone());
        self.index
            .drafts
            .insert(draft.draft_id.clone(), draft.clone());
        Ok(draft)
    }

    /// Apply an immutable, versioned draft patch. Objective operations require a second grant.
    pub fn edit_draft(&mut self, request: &FacadeRequest, patch: Patch) -> FacadeResult<Draft> {
        self.authorize(request, FacadePermission::Edit, true)?;
        self.validate_patch(&patch)?;
        let objective = self.patch_requires_objective_authority(&patch)?;
        if objective {
            self.authorize(request, FacadePermission::Objective, true)?;
        }
        self.require_owner("apply_patch")?;
        let author_ref = request.principal().to_owned();
        let draft = self
            .client
            .call(|owner, auth| {
                owner.apply_patch(
                    auth,
                    patch,
                    &author_ref,
                    if objective {
                        OwnerAuthorization::with_objective()
                    } else {
                        OwnerAuthorization::ordinary()
                    },
                )
            })
            .map_err(FacadeError::owner)
            .and_then(|draft| self.project_draft(draft))?;
        self.index.draft_ids.insert(draft.draft_id.clone());
        self.index
            .drafts
            .insert(draft.draft_id.clone(), draft.clone());
        Ok(draft)
    }

    pub fn apply_patch(&mut self, request: &FacadeRequest, patch: Patch) -> FacadeResult<Draft> {
        self.edit_draft(request, patch)
    }

    pub fn preview(
        &mut self,
        request: &FacadeRequest,
        preview: PreviewRequest,
    ) -> FacadeResult<Preview> {
        self.authorize(request, FacadePermission::Edit, true)?;
        self.validate_scope(&preview.scope)?;
        if !valid_positive_safe_integer(preview.expected_draft_version)
            || !valid_safe_integer(preview.expected_control_version)
        {
            return Err(FacadeError::invalid("invalid_preview"));
        }
        self.ensure_known_id(
            &preview.draft_id,
            &self.index.draft_ids,
            self.index.drafts_complete,
        )?;
        if preview.applicable_requested {
            self.authorize(request, FacadePermission::Pause, true)?;
        }
        self.require_owner("create_preview")?;
        let scope = preview.scope.clone();
        let draft_id = preview.draft_id.clone();
        let result = self
            .client
            .call(|owner, auth| {
                owner.create_preview(
                    auth,
                    scope,
                    &draft_id,
                    preview.expected_draft_version,
                    preview.applicable_requested,
                    preview.expected_control_version,
                    preview.unknown_total_risk_acknowledged,
                )
            })
            .map_err(FacadeError::owner)
            .and_then(|value| self.project_preview(value))?;
        self.index.preview_ids.insert(result.preview_id.clone());
        Ok(result)
    }

    pub fn pause(&mut self, request: &FacadeRequest, command: Command) -> FacadeResult<Receipt> {
        self.authorize(request, FacadePermission::Pause, true)?;
        self.validate_command(&command, "pause")?;
        self.require_owner("pause")?;
        let receipt = self
            .client
            .call(|owner, auth| owner.pause(auth, command))
            .map_err(FacadeError::owner)
            .and_then(|receipt| self.project_receipt(receipt))?;
        self.index.receipt_ids.insert(receipt.command_id.clone());
        self.index
            .revision_ids
            .insert(receipt.active_revision_id.clone());
        Ok(receipt)
    }

    /// Commit is deliberately separate from resume. The owner must already be held and the
    /// caller must provide the exact owner-issued prepared manifest digest.
    pub fn held_commit(
        &mut self,
        request: &FacadeRequest,
        command: Command,
    ) -> FacadeResult<Receipt> {
        self.authorize(request, FacadePermission::Commit, true)?;
        self.validate_command(&command, "commit")?;
        let preview_id = command
            .preview_id
            .as_deref()
            .ok_or_else(|| FacadeError::invalid("preview_required"))?;
        self.ensure_known_id(
            preview_id,
            &self.index.preview_ids,
            self.index.previews_complete,
        )?;
        if command.approved_manifest_sha256.is_none() {
            return Err(FacadeError::invalid("owner_manifest_required"));
        }
        self.require_owner("commit")?;
        let receipt = self
            .client
            .call(|owner, auth| owner.commit(auth, command))
            .map_err(FacadeError::owner)
            .and_then(|receipt| self.project_receipt(receipt))?;
        self.index.receipt_ids.insert(receipt.command_id.clone());
        self.index
            .revision_ids
            .insert(receipt.active_revision_id.clone());
        Ok(receipt)
    }

    pub fn commit(&mut self, request: &FacadeRequest, command: Command) -> FacadeResult<Receipt> {
        self.held_commit(request, command)
    }

    pub fn resume(&mut self, request: &FacadeRequest, command: Command) -> FacadeResult<Receipt> {
        self.authorize(request, FacadePermission::Resume, true)?;
        self.validate_command(&command, "resume")?;
        if let Some(preview_id) = command.expected_preview_id.as_deref() {
            self.ensure_known_id(
                preview_id,
                &self.index.preview_ids,
                self.index.previews_complete,
            )?;
        }
        self.require_owner("resume")?;
        let receipt = self
            .client
            .call(|owner, auth| owner.resume(auth, command))
            .map_err(FacadeError::owner)
            .and_then(|receipt| self.project_receipt(receipt))?;
        self.index.receipt_ids.insert(receipt.command_id.clone());
        self.index
            .revision_ids
            .insert(receipt.active_revision_id.clone());
        Ok(receipt)
    }

    fn authorize(
        &self,
        request: &FacadeRequest,
        permission: FacadePermission,
        write: bool,
    ) -> FacadeResult<()> {
        if !valid_id(request.principal()) || request.principal().len() > MAX_FACADE_PRINCIPAL_BYTES
        {
            return Err(FacadeError::invalid("invalid_principal"));
        }
        if request.token.len() > MAX_FACADE_TOKEN_BYTES
            || request.host.len() > MAX_FACADE_HOST_BYTES
            || request
                .origin
                .as_deref()
                .is_some_and(|origin| origin.len() > MAX_FACADE_ORIGIN_BYTES)
        {
            return Err(FacadeError::invalid("request_bounds"));
        }
        if request.host() != self.config.expected_host {
            return Err(FacadeError::denied("host_not_allowed"));
        }
        if self.config.expected_origin.as_deref() != request.origin() {
            return Err(FacadeError::denied("origin_not_allowed"));
        }
        if write {
            let Some(digest) = self.config.csrf_digest else {
                return Err(FacadeError::denied("csrf_unconfigured"));
            };
            let Some(csrf) = request.csrf_token.as_deref() else {
                return Err(FacadeError::denied("csrf_rejected"));
            };
            if csrf.len() > MAX_FACADE_TOKEN_BYTES {
                return Err(FacadeError::denied("csrf_rejected"));
            }
            if !digest.matches(csrf) {
                return Err(FacadeError::denied("csrf_rejected"));
            }
        }
        self.grants
            .authorize(&request.token, permission, &self.config.scope, request.now)
            .map_err(|error| FacadeError::grant(permission, error))
    }

    fn validate_scope(&self, scope: &Scope) -> FacadeResult<()> {
        if scope != &self.config.scope {
            Err(FacadeError::denied("scope_mismatch"))
        } else {
            Ok(())
        }
    }

    fn validate_patch(&mut self, patch: &Patch) -> FacadeResult<()> {
        if patch.schema != "ascension.context-control.patch.v1"
            || patch.operations.is_empty()
            || patch.operations.len() > crate::control::MAX_OPERATIONS
            || !valid_positive_safe_integer(patch.expected_draft_version)
        {
            return Err(FacadeError::invalid("invalid_patch"));
        }
        self.validate_scope(&patch.scope)?;
        self.ensure_known_id(
            &patch.draft_id,
            &self.index.draft_ids,
            self.index.drafts_complete,
        )?;
        self.ensure_current_draft_version(&patch.draft_id, patch.expected_draft_version)?;
        self.ensure_known_id(
            &patch.expected_active_revision_id,
            &self.index.revision_ids,
            self.index.revisions_complete,
        )?;
        for operation in &patch.operations {
            use crate::control::Operation;
            match operation {
                Operation::IncludeItem { item }
                | Operation::ExcludeItem { item }
                | Operation::PinItem { item }
                | Operation::UnpinItem { item } => self.ensure_item(item)?,
                Operation::PutNote {
                    note_id,
                    text,
                    expires_at,
                    expected_note_version,
                } => {
                    if !valid_id(note_id)
                        || text.is_empty()
                        || text.len() > crate::control::MAX_NOTE_BYTES
                        || text.contains('\0')
                        || !valid_timestamp(expires_at)
                        || expected_note_version.is_some_and(|version| !valid_safe_integer(version))
                    {
                        return Err(FacadeError::invalid("invalid_note"));
                    }
                }
                Operation::RemoveNote {
                    note_id,
                    expected_note_version,
                } => {
                    if !valid_id(note_id) {
                        return Err(FacadeError::invalid("invalid_note"));
                    }
                    if !valid_safe_integer(*expected_note_version) {
                        return Err(FacadeError::invalid("invalid_note"));
                    }
                    let Some(draft) = self.index.drafts.get(&patch.draft_id) else {
                        return Err(FacadeError::owner(OwnerError::Unsupported));
                    };
                    if !draft.note_items.iter().any(|item| item.item_id == *note_id) {
                        return Err(FacadeError::denied("foreign_reference"));
                    }
                }
                Operation::SetObjective { text } => {
                    if text.is_empty()
                        || text.len() > crate::control::MAX_OBJECTIVE_BYTES
                        || text.contains('\0')
                    {
                        return Err(FacadeError::invalid("invalid_objective"));
                    }
                }
                Operation::RestoreConfiguration { source_revision_id } => {
                    self.ensure_known_id(
                        source_revision_id,
                        &self.index.revision_ids,
                        self.index.revisions_complete,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn patch_requires_objective_authority(&self, patch: &Patch) -> FacadeResult<bool> {
        if patch
            .operations
            .iter()
            .any(|operation| matches!(operation, crate::control::Operation::SetObjective { .. }))
        {
            return Ok(true);
        }
        let Some(operation) = patch.operations.iter().find(|operation| {
            matches!(
                operation,
                crate::control::Operation::RestoreConfiguration { .. }
            )
        }) else {
            return Ok(false);
        };
        let crate::control::Operation::RestoreConfiguration { source_revision_id } = operation
        else {
            return Ok(false);
        };
        let Some(current_draft) = self.index.drafts.get(&patch.draft_id) else {
            return Err(FacadeError::owner(OwnerError::Unsupported));
        };
        let Some(source_revision) = self.index.revisions.get(source_revision_id) else {
            return Err(FacadeError::owner(OwnerError::Unsupported));
        };
        Ok(current_draft.objective_item != source_revision.objective_item)
    }

    fn ensure_current_draft_version(
        &self,
        draft_id: &str,
        expected_draft_version: u64,
    ) -> FacadeResult<()> {
        let Some(current) = self.index.drafts.get(draft_id) else {
            return Err(FacadeError::owner(OwnerError::Unsupported));
        };
        if current.version != expected_draft_version {
            return Err(FacadeError::stale("stale_draft"));
        }
        Ok(())
    }

    fn validate_command(&self, command: &Command, kind: &str) -> FacadeResult<()> {
        if command.schema != "ascension.context-control.command.v1"
            || command.kind != kind
            || !valid_id(&command.idempotency_key)
            || !valid_id(&command.command_window_id)
            || !valid_safe_integer(command.expected_control_version)
        {
            return Err(FacadeError::invalid("invalid_command"));
        }
        self.validate_scope(&command.scope)?;
        if let Some(revision_id) = command.expected_active_revision_id.as_deref() {
            self.ensure_known_id(
                revision_id,
                &self.index.revision_ids,
                self.index.revisions_complete,
            )?;
        }
        for reference in [
            command.preview_id.as_deref(),
            command.expected_preview_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            self.ensure_known_id(
                reference,
                &self.index.preview_ids,
                self.index.previews_complete,
            )?;
        }
        if let Some(digest) = command.approved_manifest_sha256.as_deref()
            && !valid_digest(digest)
        {
            return Err(FacadeError::invalid("invalid_reference"));
        }
        Ok(())
    }

    fn require_owner(&self, operation: &str) -> FacadeResult<()> {
        if let Some(capabilities) = &self.owner_capabilities {
            if !capabilities.enabled {
                return Err(FacadeError::denied("owner_disabled"));
            }
            if owner_supports_list(&capabilities.supported_operations, operation) {
                return Ok(());
            }
            return Err(FacadeError::owner(OwnerError::Unsupported));
        }
        Err(FacadeError::owner(
            self.owner_capability_error.unwrap_or(OwnerError::Unknown),
        ))
    }

    fn load_owner_capabilities(&mut self) {
        let scope = self.config.scope.clone();
        match self
            .client
            .call(|owner, auth| owner.capabilities(auth, &scope))
        {
            Ok(capabilities) if valid_owner_capabilities(&capabilities, &self.config.scope) => {
                self.owner_capabilities = Some(capabilities);
                self.owner_capability_error = None;
            }
            Ok(capabilities) if capabilities.scope != self.config.scope => {
                self.owner_capabilities = None;
                self.owner_capability_error = Some(OwnerError::Denied);
            }
            Ok(_) => {
                self.owner_capabilities = None;
                self.owner_capability_error = Some(OwnerError::Invalid);
            }
            Err(error) => {
                self.owner_capabilities = None;
                self.owner_capability_error = Some(error);
            }
        }
    }

    fn refresh_reference_index(&mut self) {
        // Never retain an old owner's IDs across a capability/owner refresh.  A failed refresh
        // therefore fails closed instead of allowing a stale reference to reach a new owner.
        self.index = ReferenceIndex::default();
        let Some(capabilities) = self.owner_capabilities.clone() else {
            return;
        };
        if owner_supports_list(&capabilities.supported_operations, "eligible_items") {
            let scope = self.config.scope.clone();
            match self
                .client
                .call(|owner, auth| owner.eligible_items(auth, &scope))
            {
                Ok(items) => match self.project_items(items, false) {
                    Ok(items) => {
                        self.index.item_refs.clear();
                        for item in items {
                            self.index
                                .item_refs
                                .insert((item.item.item_id.clone(), item.item.version), item.item);
                        }
                        self.index.items_complete = true;
                    }
                    Err(_) => self.index.items_complete = false,
                },
                Err(_) => self.index.items_complete = false,
            }
        }
        if owner_supports_list(&capabilities.supported_operations, "revisions") {
            let scope = self.config.scope.clone();
            match self
                .client
                .call(|owner, auth| owner.revisions(auth, &scope))
            {
                Ok(revisions) => match self.project_revisions(revisions) {
                    Ok(revisions) => {
                        self.index.revision_ids = revisions
                            .iter()
                            .map(|revision| revision.revision_id.clone())
                            .collect();
                        self.index.revisions = revisions
                            .into_iter()
                            .map(|revision| (revision.revision_id.clone(), revision))
                            .collect();
                        self.index.revisions_complete = true;
                    }
                    Err(_) => self.index.revisions_complete = false,
                },
                Err(_) => self.index.revisions_complete = false,
            }
        }
        if owner_supports_list(&capabilities.supported_operations, "drafts") {
            let scope = self.config.scope.clone();
            match self.client.call(|owner, auth| owner.drafts(auth, &scope)) {
                Ok(drafts) => match self.project_drafts(drafts) {
                    Ok(drafts) => {
                        self.index.drafts.clear();
                        self.index.draft_ids = drafts
                            .into_iter()
                            .map(|draft| {
                                self.index
                                    .drafts
                                    .insert(draft.draft_id.clone(), draft.clone());
                                draft.draft_id
                            })
                            .collect();
                        self.index.drafts_complete = true;
                    }
                    Err(_) => self.index.drafts_complete = false,
                },
                Err(_) => self.index.drafts_complete = false,
            }
        }
        if owner_supports_list(&capabilities.supported_operations, "previews") {
            let scope = self.config.scope.clone();
            match self.client.call(|owner, auth| owner.previews(auth, &scope)) {
                Ok(previews) => match self.project_previews(previews) {
                    Ok(previews) => {
                        self.index.preview_ids = previews
                            .into_iter()
                            .map(|preview| preview.preview_id)
                            .collect();
                        self.index.previews_complete = true;
                    }
                    Err(_) => self.index.previews_complete = false,
                },
                Err(_) => self.index.previews_complete = false,
            }
        }
        if owner_supports_list(&capabilities.supported_operations, "receipts") {
            let scope = self.config.scope.clone();
            match self.client.call(|owner, auth| owner.receipts(auth, &scope)) {
                Ok(receipts) => match self.project_receipts(receipts) {
                    Ok(receipts) => {
                        self.index.receipt_ids = receipts
                            .into_iter()
                            .map(|receipt| receipt.command_id)
                            .collect();
                        self.index.receipts_complete = true;
                    }
                    Err(_) => self.index.receipts_complete = false,
                },
                Err(_) => self.index.receipts_complete = false,
            }
        }
    }

    fn ensure_known_id(
        &self,
        id: &str,
        known: &BTreeSet<String>,
        complete: bool,
    ) -> FacadeResult<()> {
        if !valid_id(id) {
            return Err(FacadeError::invalid("invalid_reference"));
        }
        if known.contains(id) {
            Ok(())
        } else if complete {
            Err(FacadeError::denied("foreign_reference"))
        } else {
            Err(FacadeError::owner(OwnerError::Unsupported))
        }
    }

    fn ensure_item(&self, item: &ItemRef) -> FacadeResult<()> {
        if !valid_item_ref(item) {
            return Err(FacadeError::invalid("invalid_reference"));
        }
        match self
            .index
            .item_refs
            .get(&(item.item_id.clone(), item.version))
        {
            Some(known) if known == item => Ok(()),
            Some(_) => Err(FacadeError::denied("foreign_reference")),
            None if self.index.items_complete => Err(FacadeError::denied("foreign_reference")),
            None => Err(FacadeError::owner(OwnerError::Unsupported)),
        }
    }

    fn scoped_state(&self, state: State) -> FacadeResult<State> {
        self.validate_scope(&state.scope)?;
        if state.schema != crate::control::STATE_SCHEMA
            || !STATE_STATUSES.contains(&state.status.as_str())
            || !valid_safe_integer(state.control_version)
            || !valid_positive_safe_integer(state.controller_epoch)
            || !valid_safe_integer(state.gate_epoch)
            || !valid_safe_integer(state.plan_epoch)
            || !valid_id(&state.active_revision_id)
            || !valid_id(&state.command_window_id)
            || !valid_timestamp(&state.command_window_expires_at)
            || state.outstanding_provider_attempts.len() > crate::control::MAX_ITEMS
            || state.unresolved_operations.len() > crate::control::MAX_ITEMS
            || state.last_sequence > MAX_SAFE_INTEGER
            || state
                .outstanding_provider_attempts
                .iter()
                .chain(state.unresolved_operations.iter())
                .any(|id| !valid_id(id))
            || state
                .boundary
                .as_ref()
                .is_some_and(|boundary| self.validate_boundary(boundary).is_err())
        {
            return Err(FacadeError::invalid("owner_invalid_state"));
        }
        Ok(state)
    }

    fn validate_boundary(&self, boundary: &Boundary) -> FacadeResult<()> {
        self.validate_scope(&boundary.scope)?;
        if !valid_id(&boundary.state_id)
            || !valid_safe_integer(boundary.generation)
            || !valid_digest(&boundary.observation_sha256)
            || !valid_digest(&boundary.catalog_sha256)
            || !valid_positive_safe_integer(boundary.controller_epoch)
            || !valid_safe_integer(boundary.gate_epoch)
            || !valid_safe_integer(boundary.control_version)
            || !valid_safe_integer(boundary.lease_epoch)
            || !valid_id(&boundary.adapter_revision)
            || !valid_digest(&boundary.adapter_sha256)
            || !valid_id(&boundary.model)
            || !valid_digest(&boundary.configuration_sha256)
            || !valid_digest(&boundary.output_schema_sha256)
            || !valid_digest(&boundary.capabilities_sha256)
            || !valid_id(&boundary.authorization_policy_version)
        {
            return Err(FacadeError::invalid("owner_invalid_boundary"));
        }
        Ok(())
    }

    fn project_items(
        &self,
        items: Vec<EligibleItem>,
        include_content: bool,
    ) -> FacadeResult<Vec<EligibleItem>> {
        if items.len() > crate::control::MAX_ITEMS {
            return Err(FacadeError::invalid("owner_items_too_large"));
        }
        let mut projected = Vec::with_capacity(items.len());
        let mut content_bytes = 0_usize;
        for mut item in items {
            self.validate_scope(&item.scope)?;
            if !valid_item_ref(&item.item)
                || !ELIGIBLE_ITEM_KINDS.contains(&item.kind.as_str())
                || item.bytes > crate::control::MAX_COMPONENT_BYTES
                || !valid_timestamp(&item.expires_at)
            {
                return Err(FacadeError::invalid("owner_invalid_item"));
            }
            if let Some(content) = item.content.as_deref()
                && (content.len() > crate::control::MAX_COMPONENT_BYTES
                    || content.len() != item.bytes
                    || !item.content_available
                    || item.protected
                    || digest_text(content) != item.item.sha256)
            {
                return Err(FacadeError::invalid("owner_invalid_item"));
            }
            if item.content.as_deref().is_some_and(|content| {
                content.contains('\0') || content.len() > crate::control::MAX_COMPONENT_BYTES
            }) {
                return Err(FacadeError::invalid("owner_invalid_item"));
            }
            if include_content {
                content_bytes =
                    content_bytes.saturating_add(item.content.as_ref().map_or(0, String::len));
            }
            if include_content && content_bytes > MAX_HTTP_BODY_BYTES {
                return Err(FacadeError::invalid("owner_items_too_large"));
            }
            item.locked_reason = item
                .locked_reason
                .filter(|reason| valid_locked_reason(reason));
            if !include_content || item.protected || !item.content_available {
                item.content = None;
            }
            projected.push(item);
        }
        Ok(projected)
    }

    fn project_revisions(&self, revisions: Vec<Revision>) -> FacadeResult<Vec<Revision>> {
        if revisions.len() > crate::control::MAX_ITEMS {
            return Err(FacadeError::invalid("owner_revisions_too_large"));
        }
        for revision in &revisions {
            self.validate_scope(&revision.scope)?;
            if revision.schema != crate::control::REVISION_SCHEMA
                || !valid_id(&revision.revision_id)
                || !valid_id(&revision.parent_revision_id)
                || revision.sequence == 0
                || revision.sequence > MAX_SAFE_INTEGER
                || revision.selected_items.len() > crate::control::MAX_ITEMS
                || revision.note_items.len() > crate::control::MAX_NOTES
                || revision
                    .selected_items
                    .iter()
                    .chain(revision.note_items.iter())
                    .any(|item| !valid_item_ref(item))
                || !unique_item_refs(&revision.selected_items)
                || !unique_item_refs(&revision.note_items)
                || revision
                    .pinned_item_ids
                    .iter()
                    .any(|item_id| !valid_id(item_id))
                || !unique_strings(&revision.pinned_item_ids)
                || !revision.pinned_item_ids.iter().all(|item_id| {
                    revision
                        .selected_items
                        .iter()
                        .any(|item| item.item_id == *item_id)
                })
                || revision
                    .objective_item
                    .as_ref()
                    .is_some_and(|item| !valid_item_ref(item))
                || !valid_id(&revision.approved_preview_id)
                || !valid_digest(&revision.approved_manifest_sha256)
                || !valid_id(&revision.author_ref)
                || !valid_id(&revision.intervention_id)
                || !valid_timestamp(&revision.committed_at)
                || revision.plan_epoch > MAX_SAFE_INTEGER
                || revision.state_after_commit != "paused_committed"
            {
                return Err(FacadeError::invalid("owner_invalid_revision"));
            }
        }
        Ok(revisions)
    }

    fn project_drafts(&self, drafts: Vec<Draft>) -> FacadeResult<Vec<Draft>> {
        if drafts.len() > crate::control::MAX_ITEMS {
            return Err(FacadeError::invalid("owner_drafts_too_large"));
        }
        for draft in &drafts {
            self.validate_scope(&draft.scope)?;
            self.validate_draft(draft)?;
        }
        Ok(drafts)
    }

    fn project_draft(&self, draft: Draft) -> FacadeResult<Draft> {
        self.validate_scope(&draft.scope)?;
        self.validate_draft(&draft)?;
        Ok(draft)
    }

    fn validate_draft(&self, draft: &Draft) -> FacadeResult<()> {
        if draft.schema != crate::control::DRAFT_SCHEMA
            || !valid_id(&draft.draft_id)
            || !valid_id(&draft.base_revision_id)
            || draft.version == 0
            || draft.version > MAX_SAFE_INTEGER
            || draft.selected_items.len() > crate::control::MAX_ITEMS
            || draft.note_items.len() > crate::control::MAX_NOTES
            || draft
                .selected_items
                .iter()
                .chain(draft.note_items.iter())
                .any(|item| !valid_item_ref(item))
            || !unique_item_refs(&draft.selected_items)
            || !unique_item_refs(&draft.note_items)
            || draft
                .pinned_item_ids
                .iter()
                .any(|item_id| !valid_id(item_id))
            || !unique_strings(&draft.pinned_item_ids)
            || !draft.pinned_item_ids.iter().all(|item_id| {
                draft
                    .selected_items
                    .iter()
                    .any(|item| item.item_id == *item_id)
            })
            || draft
                .objective_item
                .as_ref()
                .is_some_and(|item| !valid_item_ref(item))
            || !valid_timestamp(&draft.expires_at)
            || !valid_id(&draft.author_ref)
        {
            return Err(FacadeError::invalid("owner_invalid_draft"));
        }
        Ok(())
    }

    fn project_previews(&self, previews: Vec<Preview>) -> FacadeResult<Vec<Preview>> {
        if previews.len() > crate::control::MAX_ITEMS {
            return Err(FacadeError::invalid("owner_previews_too_large"));
        }
        for preview in &previews {
            self.project_preview(preview.clone())?;
        }
        Ok(previews)
    }

    fn project_preview(&self, preview: Preview) -> FacadeResult<Preview> {
        self.validate_scope(&preview.scope)?;
        self.validate_boundary(&preview.boundary)?;
        if preview.schema != crate::control::PREVIEW_SCHEMA
            || !valid_id(&preview.preview_id)
            || !valid_id(&preview.draft_id)
            || !valid_id(&preview.base_revision_id)
            || preview.draft_version == 0
            || preview.draft_version > MAX_SAFE_INTEGER
            || preview.blockers.len() > MAX_FACADE_BLOCKERS
            || preview.blockers.iter().any(|blocker| !valid_id(blocker))
            || !unique_strings(&preview.blockers)
            || preview.components.len() > MAX_FACADE_COMPONENTS
            || preview.selected_items.len() > crate::control::MAX_ITEMS
            || preview
                .selected_items
                .iter()
                .any(|item| !valid_item_ref(item))
            || !unique_item_refs(&preview.selected_items)
            || preview
                .prepared_manifest_sha256
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
            || preview
                .model_execution_id
                .as_deref()
                .is_some_and(|id| !valid_id(id))
            || preview
                .provider_attempt_id
                .as_deref()
                .is_some_and(|id| !valid_id(id))
            || !valid_timestamp(&preview.expires_at)
            || !matches!(
                preview.budget_status.as_str(),
                "within_known_local_limit" | "bounded_unknown_total" | "exceeded" | "unavailable"
            )
            || preview.provider_added_context != "not_exposed"
            || preview.effect_class != "local_preparation_only"
            || (preview.applicable
                && (!preview.blockers.is_empty()
                    || preview.prepared_manifest_sha256.is_none()
                    || preview.components.is_empty()
                    || preview.model_execution_id.is_none()
                    || preview.provider_attempt_id.is_none()
                    || !matches!(
                        preview.budget_status.as_str(),
                        "within_known_local_limit" | "bounded_unknown_total"
                    )
                    || (preview.budget_status == "bounded_unknown_total"
                        && !preview.unknown_total_risk_acknowledged)))
        {
            return Err(FacadeError::invalid("owner_invalid_preview"));
        }
        for component in &preview.components {
            if !valid_id(&component.component_id)
                || component.ordinal > MAX_SAFE_INTEGER
                || !matches!(
                    component.kind.as_str(),
                    "stdin"
                        | "serialized_http_body"
                        | "output_schema"
                        | "configuration"
                        | "attachment"
                )
                || !valid_digest(&component.sha256)
                || component.bytes > crate::control::MAX_COMPONENT_BYTES
                || !valid_reference(&component.content_ref)
            {
                return Err(FacadeError::invalid("owner_invalid_preview"));
            }
        }
        if !unique_strings(
            &preview
                .components
                .iter()
                .map(|component| component.component_id.clone())
                .collect::<Vec<_>>(),
        ) {
            return Err(FacadeError::invalid("owner_invalid_preview"));
        }
        // `Preview` contains only owner-issued manifests and opaque content refs.  The facade
        // never exposes the owner's prepared input bytes or synthesizes a replacement.
        Ok(preview)
    }

    fn project_receipts(&self, receipts: Vec<Receipt>) -> FacadeResult<Vec<Receipt>> {
        if receipts.len() > crate::control::MAX_ITEMS {
            return Err(FacadeError::invalid("owner_receipts_too_large"));
        }
        for receipt in &receipts {
            self.project_receipt(receipt.clone())?;
        }
        Ok(receipts)
    }

    fn project_receipt(&self, receipt: Receipt) -> FacadeResult<Receipt> {
        self.validate_scope(&receipt.scope)?;
        if receipt.schema != crate::control::RECEIPT_SCHEMA
            || !valid_id(&receipt.command_id)
            || !RECEIPT_KINDS.contains(&receipt.kind.as_str())
            || !RECEIPT_STATUSES.contains(&receipt.status.as_str())
            || !RECEIPT_EFFECTS.contains(&receipt.effect.as_str())
            || receipt.control_version > MAX_SAFE_INTEGER
            || !valid_id(&receipt.active_revision_id)
            || !valid_timestamp(&receipt.observed_at)
            || receipt
                .reason_code
                .as_deref()
                .is_some_and(|reason| !valid_id(reason))
        {
            return Err(FacadeError::invalid("owner_invalid_receipt"));
        }
        Ok(receipt)
    }
}

fn valid_owner_capabilities(capabilities: &ControlCapabilities, scope: &Scope) -> bool {
    capabilities.schema == crate::control::CAPABILITIES_SCHEMA
        && capabilities.product_phase == 2
        && capabilities.scope == *scope
        && valid_id(&capabilities.adapter_revision)
        && capabilities.supported_operations.len() <= 32
        && unique_strings(&capabilities.supported_operations)
        && capabilities
            .supported_operations
            .iter()
            .all(|operation| valid_id(operation) && known_capability_operation(operation))
        && matches!(
            capabilities.exact_application_preview.as_str(),
            "supported" | "unsupported" | "unverified"
        )
        && matches!(
            capabilities.optional_images.as_str(),
            "supported" | "unsupported" | "unverified"
        )
        && matches!(
            capabilities.durable_control_store.as_str(),
            "available" | "unavailable" | "unverified"
        )
        && capabilities.provider_added_context == "not_exposed"
        && !capabilities.context_compact
        && !capabilities.persistent_provider_sessions
        && !capabilities.direct_game_dispatch
        && !capabilities.commit_auto_resumes
}

fn validate_facade_capabilities(capabilities: &FacadeCapabilities) -> FacadeResult<()> {
    if capabilities.schema != FACADE_CAPABILITIES_SCHEMA
        || capabilities.composition != "harness_backed"
        || !scope_valid(&capabilities.scope)
        || capabilities.owner_supported_operations.len() > 32
        || !capabilities
            .owner_supported_operations
            .iter()
            .all(|operation| valid_id(operation) && known_owner_operation(operation))
        || !unique_strings(&capabilities.owner_supported_operations)
        || capabilities.forwarded_operations.len()
            > OWNER_READ_CAPABILITIES.len() + OWNER_CONTROL_CAPABILITIES.len()
        || !capabilities.forwarded_operations.iter().all(|operation| {
            OWNER_READ_CAPABILITIES
                .iter()
                .chain(OWNER_CONTROL_CAPABILITIES)
                .any(|known| known == operation)
        })
        || !unique_strings(&capabilities.forwarded_operations)
        || capabilities.grant_permissions.len() > 7
        || !capabilities.grant_permissions.iter().all(|permission| {
            FacadePermission::ALL
                .iter()
                .any(|known| known == permission)
        })
        || !unique_strings(&capabilities.grant_permissions)
        || !matches!(
            capabilities.retention_mode,
            RetentionMode::Off
                | RetentionMode::Metadata
                | RetentionMode::Memory
                | RetentionMode::PrivateEncrypted
        )
        || !matches!(
            capabilities.exact_application_preview.as_str(),
            "owner_conditional" | "unavailable"
        )
        || capabilities.provider_added_context != "unknown"
        || capabilities.direct_game_dispatch
        || capabilities.duplicate_scheduler
        || capabilities.legacy_demo
    {
        return Err(FacadeError::invalid("facade_capabilities_invalid"));
    }
    Ok(())
}

fn owner_supports_list(supported: &[String], operation: &str) -> bool {
    let aliases: &[&str] = match operation {
        "capabilities" => &["capabilities"],
        "state" => &["state"],
        "eligible_items" => &["eligible_items", "eligible-items", "eligible"],
        "revisions" => &["revisions"],
        "drafts" => &["drafts"],
        "previews" => &["previews"],
        "receipts" => &["receipts", "receipt_read"],
        // Read collection capabilities do not imply their mutation counterparts.  An owner
        // must advertise each write explicitly (or use its versioned write alias) before the
        // facade forwards it.
        "create_draft" => &["create_draft", "draft_create"],
        "apply_patch" => &["apply_patch", "draft_edit"],
        "create_preview" => &["create_preview", "preview"],
        "pause" => &["pause"],
        "commit" => &["commit"],
        "resume" => &["resume"],
        _ => &[],
    };
    aliases
        .iter()
        .any(|alias| supported.iter().any(|value| value == alias))
}

fn known_owner_operation(operation: &str) -> bool {
    OWNER_READ_CAPABILITIES
        .iter()
        .chain(OWNER_CONTROL_CAPABILITIES)
        .any(|known| operation == *known)
        || matches!(
            operation,
            "eligible-items"
                | "eligible"
                | "draft_create"
                | "draft_edit"
                | "preview"
                | "receipt_read"
        )
}

fn known_capability_operation(operation: &str) -> bool {
    known_owner_operation(operation)
        || matches!(
            operation,
            "include_item"
                | "exclude_item"
                | "pin_item"
                | "unpin_item"
                | "put_note"
                | "remove_note"
                | "set_objective"
                | "restore_configuration"
        )
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FACADE_ID_BYTES
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

fn valid_safe_integer(value: u64) -> bool {
    value <= MAX_SAFE_INTEGER
}

fn valid_item_ref(item: &ItemRef) -> bool {
    item.valid() && valid_safe_integer(item.version)
}

fn valid_positive_safe_integer(value: u64) -> bool {
    value > 0 && valid_safe_integer(value)
}

fn unique_strings(values: &[String]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().all(|value| seen.insert(value))
}

fn unique_item_refs(values: &[ItemRef]) -> bool {
    let mut seen = BTreeSet::new();
    values.iter().all(|value| seen.insert(value))
}

fn valid_locked_reason(value: &str) -> bool {
    value.len() <= MAX_FACADE_LOCKED_REASON_BYTES && LOCKED_REASON_ALLOWLIST.contains(&value)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn digest_text(value: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn scope_valid(scope: &Scope) -> bool {
    [
        scope.project_id.as_str(),
        scope.run_id.as_str(),
        scope.episode_id.as_str(),
        scope.agent_id.as_str(),
    ]
    .into_iter()
    .all(valid_id)
}

fn valid_reference(value: &str) -> bool {
    valid_id(value)
        && !value.contains("://")
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('@')
        && !value.contains('?')
        && !value.contains('#')
}

fn valid_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FACADE_HOST_BYTES
        && !value.chars().any(char::is_whitespace)
        && !value.contains(['/', '\\', '%', '@', '?', '#'])
}

fn valid_origin(value: &str) -> bool {
    if value.len() > MAX_FACADE_ORIGIN_BYTES
        || value.chars().any(char::is_whitespace)
        || !(value.starts_with("http://") || value.starts_with("https://"))
        || value.contains(['\\', '%', '@', '?', '#'])
    {
        return false;
    }
    let Some(authority) = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
    else {
        return false;
    };
    !authority.is_empty() && !authority.contains('/')
}

fn valid_timestamp(value: &str) -> bool {
    if value.len() > 64 || !value.ends_with('Z') || value.contains(['\0', '/', '\\']) {
        return false;
    }
    let Some((date, clock)) = value[..value.len() - 1].split_once('T') else {
        return false;
    };
    let date_parts = date.split('-').collect::<Vec<_>>();
    if date_parts.len() != 3
        || date_parts[0].len() != 4
        || date_parts[1].len() != 2
        || date_parts[2].len() != 2
        || date_parts
            .iter()
            .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return false;
    }
    let Ok(year) = date_parts[0].parse::<u16>() else {
        return false;
    };
    let Ok(month) = date_parts[1].parse::<u8>() else {
        return false;
    };
    let Ok(day) = date_parts[2].parse::<u8>() else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > days_in_month {
        return false;
    }
    let clock_parts = clock.split(':').collect::<Vec<_>>();
    if clock_parts.len() != 3
        || clock_parts[0].len() != 2
        || clock_parts[1].len() != 2
        || clock_parts
            .iter()
            .take(2)
            .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return false;
    }
    let seconds = clock_parts[2].split('.').collect::<Vec<_>>();
    if seconds.len() > 2
        || seconds[0].len() != 2
        || !seconds[0].bytes().all(|byte| byte.is_ascii_digit())
        || seconds.get(1).is_some_and(|fraction| {
            fraction.is_empty()
                || fraction.len() > 9
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return false;
    }
    let Ok(hour) = clock_parts[0].parse::<u8>() else {
        return false;
    };
    let Ok(minute) = clock_parts[1].parse::<u8>() else {
        return false;
    };
    let Ok(second) = seconds[0].parse::<u8>() else {
        return false;
    };
    hour < 24 && minute < 60 && second < 60
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        });
    difference == 0
}

fn parse_facade_json<T: DeserializeOwned>(body: &[u8]) -> FacadeResult<T> {
    crate::parse_control_json(body).map_err(|_| FacadeError::invalid("invalid_json"))
}

fn to_value<T: Serialize>(value: T) -> FacadeResult<serde_json::Value> {
    let bytes = serde_json::to_vec(&value).map_err(|_| FacadeError::invalid("encoding"))?;
    if bytes.len() > MAX_FACADE_RESPONSE_BYTES {
        return Err(FacadeError::response_too_large());
    }
    serde_json::from_slice(&bytes).map_err(|_| FacadeError::invalid("encoding"))
}

fn facade_http_result<T: Serialize>(result: FacadeResult<T>) -> HttpResponse {
    match result {
        Ok(value) => {
            let body = match serde_json::to_vec(&value) {
                Ok(body) if body.len() <= MAX_FACADE_RESPONSE_BYTES => body,
                Ok(_) => return facade_error_http_response(FacadeError::response_too_large()),
                Err(_) => return facade_error_http_response(FacadeError::invalid("encoding")),
            };
            HttpResponse::raw_json(200, body)
        }
        Err(error) => facade_error_http_response(error),
    }
}

fn facade_http_error(status: u16, code: &'static str) -> HttpResponse {
    HttpResponse::json(
        status,
        serde_json::json!({
            "schema": FACADE_ERROR_SCHEMA,
            "error": {
                "code": code,
                "retryable": false
            }
        }),
    )
}

fn facade_error_http_response(error: FacadeError) -> HttpResponse {
    let status = if error.code == "route_not_found" {
        404
    } else {
        match error.class {
            FacadeErrorClass::Invalid => 400,
            FacadeErrorClass::Unauthorized => 401,
            FacadeErrorClass::Denied => 403,
            FacadeErrorClass::NotFound => 404,
            FacadeErrorClass::Stale => 409,
            FacadeErrorClass::Expired => 410,
            FacadeErrorClass::Unavailable | FacadeErrorClass::Unknown => 503,
        }
    };
    HttpResponse::json(
        status,
        serde_json::json!({
            "schema": FACADE_ERROR_SCHEMA,
            "error": {
                "code": error.code,
                "retryable": error.retryable
            }
        }),
    )
}

fn duplicate_security_header(request: &HttpRequest, name: &str) -> bool {
    request
        .headers
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .nth(1)
        .is_some()
}

impl HarnessOwnerPort for crate::control::ControlPlane {
    fn capabilities(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<ControlCapabilities, OwnerError> {
        if self.scope() != scope {
            return Err(OwnerError::Denied);
        }
        let mut capabilities = crate::control::ControlPlane::capabilities(self);
        if capabilities.enabled {
            for operation in OWNER_READ_CAPABILITIES
                .iter()
                .chain(OWNER_CONTROL_CAPABILITIES)
            {
                if !capabilities
                    .supported_operations
                    .iter()
                    .any(|current| current == operation)
                {
                    capabilities
                        .supported_operations
                        .push((*operation).to_owned());
                }
            }
        }
        Ok(capabilities)
    }

    fn state(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<State, OwnerError> {
        (self.scope() == scope)
            .then_some(crate::control::ControlPlane::state(self))
            .ok_or(OwnerError::Denied)
    }

    fn eligible_items(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<EligibleItem>, OwnerError> {
        (self.scope() == scope)
            .then_some(crate::control::ControlPlane::eligible_items(self))
            .ok_or(OwnerError::Denied)
    }

    fn revisions(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Revision>, OwnerError> {
        (self.scope() == scope)
            .then_some(crate::control::ControlPlane::revisions(self))
            .ok_or(OwnerError::Denied)
    }

    fn drafts(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Draft>, OwnerError> {
        (self.scope() == scope)
            .then_some(crate::control::ControlPlane::drafts(self))
            .ok_or(OwnerError::Denied)
    }

    fn previews(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Preview>, OwnerError> {
        (self.scope() == scope)
            .then_some(crate::control::ControlPlane::previews(self))
            .ok_or(OwnerError::Denied)
    }

    fn receipts(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
    ) -> Result<Vec<Receipt>, OwnerError> {
        (self.scope() == scope)
            .then_some(crate::control::ControlPlane::receipts(self))
            .ok_or(OwnerError::Denied)
    }

    fn get_draft(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
        draft_id: &str,
    ) -> Result<Draft, OwnerError> {
        if self.scope() != scope {
            return Err(OwnerError::Denied);
        }
        crate::control::ControlPlane::get_draft(self, draft_id).map_err(OwnerError::from_control)
    }

    fn get_preview(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
        preview_id: &str,
    ) -> Result<Preview, OwnerError> {
        if self.scope() != scope {
            return Err(OwnerError::Denied);
        }
        crate::control::ControlPlane::get_preview(self, preview_id)
            .map_err(OwnerError::from_control)
    }

    fn get_receipt(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: &Scope,
        command_id: &str,
    ) -> Result<Receipt, OwnerError> {
        if self.scope() != scope {
            return Err(OwnerError::Denied);
        }
        crate::control::ControlPlane::command(self, command_id).map_err(OwnerError::from_control)
    }

    fn create_draft(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: Scope,
        expected_active_revision_id: &str,
        author_ref: &str,
    ) -> Result<Draft, OwnerError> {
        crate::control::ControlPlane::create_draft(
            self,
            scope,
            expected_active_revision_id,
            author_ref,
        )
        .map_err(OwnerError::from_control)
    }

    fn apply_patch(
        &mut self,
        _auth: &ProtectedAuthReference,
        patch: Patch,
        author_ref: &str,
        authorization: OwnerAuthorization,
    ) -> Result<Draft, OwnerError> {
        crate::control::ControlPlane::apply_patch(
            self,
            patch,
            author_ref,
            authorization.objective_override,
        )
        .map_err(OwnerError::from_control)
    }

    fn create_preview(
        &mut self,
        _auth: &ProtectedAuthReference,
        scope: Scope,
        draft_id: &str,
        expected_draft_version: u64,
        applicable_requested: bool,
        expected_control_version: u64,
        risk_ack: bool,
    ) -> Result<Preview, OwnerError> {
        crate::control::ControlPlane::create_preview(
            self,
            scope,
            draft_id,
            expected_draft_version,
            applicable_requested,
            expected_control_version,
            risk_ack,
        )
        .map_err(OwnerError::from_control)
    }

    fn pause(
        &mut self,
        _auth: &ProtectedAuthReference,
        command: Command,
    ) -> Result<Receipt, OwnerError> {
        crate::control::ControlPlane::pause(self, command).map_err(OwnerError::from_control)
    }

    fn commit(
        &mut self,
        _auth: &ProtectedAuthReference,
        command: Command,
    ) -> Result<Receipt, OwnerError> {
        crate::control::ControlPlane::commit(self, command).map_err(OwnerError::from_control)
    }

    fn resume(
        &mut self,
        _auth: &ProtectedAuthReference,
        command: Command,
    ) -> Result<Receipt, OwnerError> {
        crate::control::ControlPlane::resume(self, command).map_err(OwnerError::from_control)
    }
}
