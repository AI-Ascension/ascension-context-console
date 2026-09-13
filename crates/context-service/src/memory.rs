// SPDX-License-Identifier: MIT

//! Target-owned, bounded facade for the harness Phase 3 memory contracts.
//!
//! The target never writes the harness corpus or scheduler directly.  This facade validates the
//! operator-facing route shape and exposes capability/status metadata; policy and source bytes
//! remain harness-owned.  It is safe to use while the feature is disabled and has no provider,
//! game, process, URL, or filesystem resolver path.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::owner::{
    HarnessOwnerComposition, OwnerError, OwnerGrantBook, OwnerOperation, OwnerRequestContext,
    owner_call, owner_scope, validate_public_value,
};

pub const MEMORY_CAPABILITIES_SCHEMA: &str = "ascension.context-memory.capabilities.v1";
pub const MEMORY_QUERY_SCHEMA: &str = "ascension.context-memory.query.v1";
pub const MAX_MEMORY_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_MEMORY_BODY_BYTES: usize = 16 * 1024;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryScope {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCapabilities {
    pub schema: String,
    pub product_phase: u8,
    pub scope: MemoryScope,
    pub enabled: bool,
    pub local_lexical_retrieval: String,
    pub extractive_compaction: String,
    pub abstractive_adapter: String,
    pub abstractive_live_verified: bool,
    pub per_decision_policy: String,
    pub phase2_approval_required: bool,
    pub persistent_provider_sessions: bool,
    pub provider_side_compaction: bool,
    pub semantic_vector_retrieval: String,
    pub hidden_reasoning_access: bool,
    pub direct_game_dispatch: bool,
    pub supported_operations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryQueryRequest {
    pub schema: String,
    pub scope: MemoryScope,
    pub branch_id: String,
    pub query: String,
    pub cutoff: u64,
    pub corpus_generation: u64,
    pub ranker_version: String,
    pub limit: usize,
    pub max_candidates: usize,
    pub effect_class: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryRouteError {
    MethodNotAllowed,
    BodyTooLarge,
    InvalidRequest,
    PermissionDenied,
    Unsupported,
    Unavailable,
}

impl std::fmt::Display for MemoryRouteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MethodNotAllowed => "memory route method is not allowed",
            Self::BodyTooLarge => "memory route body exceeds its bound",
            Self::InvalidRequest => "memory route request is invalid",
            Self::PermissionDenied => "memory route permission is denied",
            Self::Unsupported => "memory route operation is unsupported",
            Self::Unavailable => "memory route owner is unavailable",
        })
    }
}

impl std::error::Error for MemoryRouteError {}

#[derive(Clone, Debug)]
pub struct MemoryRoute {
    scope: MemoryScope,
    enabled: bool,
    search_principals: Vec<String>,
    review_principals: Vec<String>,
    owner: Option<HarnessOwnerComposition>,
}

impl PartialEq for MemoryRoute {
    fn eq(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.enabled == other.enabled
            && self.search_principals == other.search_principals
            && self.review_principals == other.review_principals
            && self.owner.is_some() == other.owner.is_some()
    }
}

impl Eq for MemoryRoute {}

impl MemoryRoute {
    pub fn new(scope: MemoryScope, enabled: bool) -> Self {
        Self {
            scope,
            enabled,
            search_principals: Vec::new(),
            review_principals: Vec::new(),
            owner: None,
        }
    }

    /// Builds the explicitly attached composition. No local corpus or generation state is
    /// created; every supported operation is forwarded to `owner`.
    #[must_use]
    pub fn attached(scope: MemoryScope, composition: HarnessOwnerComposition) -> Self {
        Self {
            scope,
            enabled: true,
            search_principals: Vec::new(),
            review_principals: Vec::new(),
            owner: Some(composition),
        }
    }

    /// Convenience constructor for integrations that do not need to retain the composition.
    #[must_use]
    pub fn with_owner(
        scope: MemoryScope,
        owner: std::sync::Arc<dyn crate::owner::HarnessOwner>,
        grants: OwnerGrantBook,
    ) -> Self {
        Self::attached(scope, HarnessOwnerComposition::new(owner, grants))
    }

    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.owner.is_some()
    }

    /// Revokes a configured attached grant for all routes sharing its composition.
    pub fn revoke_grant(&self, grant_id: &str) -> Result<(), crate::owner::OwnerAuthError> {
        self.owner
            .as_ref()
            .ok_or(crate::owner::OwnerAuthError::GrantBookUnavailable)?
            .revoke(grant_id)
    }

    pub fn grant_search(&mut self, principal: impl Into<String>) {
        let principal = principal.into();
        if !principal.is_empty() && !self.search_principals.contains(&principal) {
            self.search_principals.push(principal);
        }
    }

    pub fn grant_review(&mut self, principal: impl Into<String>) {
        let principal = principal.into();
        if !principal.is_empty() && !self.review_principals.contains(&principal) {
            self.review_principals.push(principal);
        }
    }

    pub fn capabilities(&self) -> MemoryCapabilities {
        MemoryCapabilities {
            schema: MEMORY_CAPABILITIES_SCHEMA.to_owned(),
            product_phase: 3,
            scope: self.scope.clone(),
            enabled: self.enabled,
            // The target facade has no attached harness corpus or summary adapter.  Reporting
            // these lanes as supported would make an unavailable projection look like a working
            // product capability.  Keep the endpoint discoverable while naming the attachment
            // boundary explicitly.
            local_lexical_retrieval: "unverified".to_owned(),
            extractive_compaction: "unverified".to_owned(),
            abstractive_adapter: "unsupported".to_owned(),
            abstractive_live_verified: false,
            per_decision_policy: "unverified".to_owned(),
            phase2_approval_required: true,
            persistent_provider_sessions: false,
            provider_side_compaction: false,
            semantic_vector_retrieval: "unsupported".to_owned(),
            hidden_reasoning_access: false,
            direct_game_dispatch: false,
            supported_operations: if self.enabled {
                ["search"].into_iter().map(str::to_owned).collect()
            } else {
                Vec::new()
            },
        }
    }

    pub fn handle(
        &self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if let Some(owner) = &self.owner {
            let now = unix_seconds();
            let context = owner
                .context_for(principal, now)
                .ok_or(MemoryRouteError::PermissionDenied)?;
            return self.handle_with_context(method, path, &context, body);
        }
        self.handle_local(method, path, principal, body)
    }

    /// Handles an attached request with the complete security context. Host/origin/CSRF and grant
    /// checks happen before constructing an owner call.
    pub fn handle_with_context(
        &self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if self.owner.is_some() {
            self.handle_attached(method, path, context, body)
        } else {
            self.handle_local(method, path, &context.principal, body)
        }
    }

    fn handle_local(
        &self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if body.len() > MAX_MEMORY_BODY_BYTES {
            return Err(MemoryRouteError::BodyTooLarge);
        }
        match (method, path) {
            ("GET", "/v3/memory/capabilities") => {
                if !self
                    .search_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                serde_json::to_value(self.capabilities()).map_err(|_| MemoryRouteError::Unsupported)
            }
            ("GET", "/v3/memory/status") => {
                if !self
                    .search_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                Ok(json!({
                    "schema": "ascension.context-memory.status.v1",
                    "enabled": self.enabled,
                    "corpus_generation": Value::Null,
                    "projection_generation": Value::Null,
                    "revocation_epoch": Value::Null,
                    "inference_calls": 0,
                    "effect_class": "local_read_no_inference"
                }))
            }
            ("POST", "/v3/memory/search") => {
                if !self
                    .search_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                let request: MemoryQueryRequest = crate::parse_control_json(body)
                    .map_err(|_| MemoryRouteError::InvalidRequest)?;
                if request.schema != MEMORY_QUERY_SCHEMA
                    || request.scope != self.scope
                    || !valid_id(&request.branch_id)
                    || request.query.is_empty()
                    || request.query.len() > MAX_MEMORY_QUERY_BYTES
                    || request.cutoff > MAX_SAFE_INTEGER
                    || request.corpus_generation == 0
                    || request.corpus_generation > MAX_SAFE_INTEGER
                    || !valid_id(&request.ranker_version)
                    || request.limit == 0
                    || request.limit > 16
                    || request.max_candidates == 0
                    || request.max_candidates > 64
                    || request.effect_class != "local_read_no_inference"
                {
                    return Err(MemoryRouteError::InvalidRequest);
                }
                if !self.enabled {
                    return Ok(json!({
                        "schema": "ascension.context-memory.retrieval.v1",
                        "query_id": "disabled",
                        "scope": self.scope,
                        "branch_id": request.branch_id,
                        "query_sha256": digest(request.query.as_bytes()),
                        "cutoff": request.cutoff,
                        "corpus_generation": request.corpus_generation,
                        "projection_generation": Value::Null,
                        "revocation_epoch": Value::Null,
                        "ranker_version": request.ranker_version,
                        "results": [],
                        "coverage": "projection_unavailable",
                        "inference_calls": 0
                    }));
                }
                Ok(json!({
                    "schema": "ascension.context-memory.retrieval.v1",
                    "query_id": "pending-harness-query",
                    "scope": self.scope,
                    "branch_id": request.branch_id,
                    "query_sha256": digest(request.query.as_bytes()),
                    "cutoff": request.cutoff,
                    "corpus_generation": request.corpus_generation,
                    "projection_generation": 0,
                    "revocation_epoch": 0,
                    "ranker_version": request.ranker_version,
                    "results": [],
                    "coverage": "projection_unavailable",
                    "inference_calls": 0
                }))
            }
            ("POST", "/v3/memory/generate") | ("POST", "/v3/memory/review") => {
                if !self
                    .review_principals
                    .iter()
                    .any(|value| value == principal)
                {
                    return Err(MemoryRouteError::PermissionDenied);
                }
                Err(MemoryRouteError::Unsupported)
            }
            ("GET", _) | ("POST", _) => Err(MemoryRouteError::Unsupported),
            _ => Err(MemoryRouteError::MethodNotAllowed),
        }
    }

    fn handle_attached(
        &self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, MemoryRouteError> {
        if body.len() > MAX_MEMORY_BODY_BYTES {
            return Err(MemoryRouteError::BodyTooLarge);
        }
        let operation = memory_operation(method, path, &self.scope.run_id)?;
        let write = !operation.read_only();
        let scope = owner_scope(
            &self.scope.project_id,
            &self.scope.run_id,
            &self.scope.episode_id,
            &self.scope.agent_id,
        );
        let composition = self.owner.as_ref().ok_or(MemoryRouteError::Unsupported)?;
        let grant = composition
            .authorize(context, operation.grant_class(), &scope, write)
            .map_err(|_| MemoryRouteError::PermissionDenied)?;
        let revocation_epoch = grant.revocation_epoch;
        let payload = match operation {
            OwnerOperation::MemoryCapabilities | OwnerOperation::MemoryStatus => {
                if !body.is_empty() {
                    return Err(MemoryRouteError::InvalidRequest);
                }
                json!({})
            }
            OwnerOperation::MemoryQuery => {
                let request: MemoryQueryRequest = crate::parse_control_json(body)
                    .map_err(|_| MemoryRouteError::InvalidRequest)?;
                validate_query(&request, &self.scope)?;
                serde_json::to_value(request).map_err(|_| MemoryRouteError::InvalidRequest)?
            }
            OwnerOperation::MemoryGeneration
            | OwnerOperation::MemoryReview
            | OwnerOperation::MemorySelection => parse_owner_command(body)?,
            _ => return Err(MemoryRouteError::Unsupported),
        };
        let call = owner_call(operation, scope.clone(), None, payload, grant)
            .map_err(|_| MemoryRouteError::InvalidRequest)?;
        let owner = composition.owner();
        let reply = match owner.call(call) {
            Ok(reply) => reply,
            Err(OwnerError::LostReply { receipt_id }) => {
                if !valid_id(&receipt_id) {
                    return Err(MemoryRouteError::Unavailable);
                }
                let lookup = crate::owner::OwnerReceiptLookup {
                    operation,
                    scope,
                    reference: None,
                    receipt_id: receipt_id.clone(),
                };
                match owner.lookup_receipt(lookup) {
                    Ok(reply) => reply,
                    Err(OwnerError::UnknownReceipt | OwnerError::Unavailable) => {
                        crate::owner::OwnerReply::unknown(
                            receipt_id,
                            format!("unknown-{}", operation.as_str().replace('.', "-")),
                            operation,
                            "harness",
                            0,
                            "receipt_lookup_unavailable",
                        )
                        .map_err(|_| MemoryRouteError::Unsupported)?
                    }
                    Err(OwnerError::Unsupported) => crate::owner::OwnerReply::unsupported(
                        "unsupported-receipt",
                        "unsupported",
                        operation,
                        "harness",
                        0,
                        "owner_unsupported",
                    )
                    .map_err(|_| MemoryRouteError::Unsupported)?,
                    Err(OwnerError::LostReply { .. } | OwnerError::MalformedResponse) => {
                        return Err(MemoryRouteError::Unsupported);
                    }
                }
            }
            Err(OwnerError::Unsupported) => crate::owner::OwnerReply::unsupported(
                "unsupported-receipt",
                "unsupported",
                operation,
                "harness",
                0,
                "owner_unsupported",
            )
            .map_err(|_| MemoryRouteError::Unsupported)?,
            Err(OwnerError::Unavailable | OwnerError::UnknownReceipt) => {
                return Err(MemoryRouteError::Unavailable);
            }
            Err(OwnerError::MalformedResponse) => return Err(MemoryRouteError::Unsupported),
        };
        reply
            .validate_for(operation)
            .map_err(|_| MemoryRouteError::Unsupported)?;
        owner_result(operation, revocation_epoch, reply)
    }
}

fn validate_query(
    request: &MemoryQueryRequest,
    scope: &MemoryScope,
) -> Result<(), MemoryRouteError> {
    if request.schema != MEMORY_QUERY_SCHEMA
        || request.scope != *scope
        || !valid_id(&request.branch_id)
        || request.query.is_empty()
        || request.query.len() > MAX_MEMORY_QUERY_BYTES
        || request.cutoff > MAX_SAFE_INTEGER
        || request.corpus_generation == 0
        || request.corpus_generation > MAX_SAFE_INTEGER
        || !valid_id(&request.ranker_version)
        || request.limit == 0
        || request.limit > 16
        || request.max_candidates == 0
        || request.max_candidates > 64
        || request.effect_class != "local_read_no_inference"
    {
        return Err(MemoryRouteError::InvalidRequest);
    }
    Ok(())
}

fn memory_operation(
    method: &str,
    path: &str,
    expected_run_id: &str,
) -> Result<OwnerOperation, MemoryRouteError> {
    let operation = match (method, path) {
        ("GET", "/v3/memory/capabilities") => Some(OwnerOperation::MemoryCapabilities),
        ("GET", "/v3/memory/status") => Some(OwnerOperation::MemoryStatus),
        ("POST", "/v3/memory/search") => Some(OwnerOperation::MemoryQuery),
        ("POST", "/v3/memory/generate") => Some(OwnerOperation::MemoryGeneration),
        ("POST", "/v3/memory/review") => Some(OwnerOperation::MemoryReview),
        ("POST", "/v3/memory/select" | "/v3/memory/selection") => {
            Some(OwnerOperation::MemorySelection)
        }
        _ => None,
    };
    if let Some(operation) = operation {
        return Ok(operation);
    }
    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() == 5
        && segments[0] == "v3"
        && segments[1] == "runs"
        && segments[3] == "context-memory"
        && segments[2] != expected_run_id
    {
        return Err(MemoryRouteError::PermissionDenied);
    }
    if segments.len() != 5
        || segments[0] != "v3"
        || segments[1] != "runs"
        || segments[2] != expected_run_id
        || segments[3] != "context-memory"
    {
        return Err(if method == "GET" {
            MemoryRouteError::Unsupported
        } else {
            MemoryRouteError::MethodNotAllowed
        });
    }
    match (method, segments[4]) {
        ("GET", "capabilities" | "status") => Ok(if segments[4] == "capabilities" {
            OwnerOperation::MemoryCapabilities
        } else {
            OwnerOperation::MemoryStatus
        }),
        ("POST", "search") => Ok(OwnerOperation::MemoryQuery),
        ("POST", "summary-jobs") => Ok(OwnerOperation::MemoryGeneration),
        ("POST", "reviews") => Ok(OwnerOperation::MemoryReview),
        ("POST", "selections") => Ok(OwnerOperation::MemorySelection),
        ("GET", _) => Err(MemoryRouteError::Unsupported),
        _ => Err(MemoryRouteError::MethodNotAllowed),
    }
}

fn parse_owner_command(body: &[u8]) -> Result<Value, MemoryRouteError> {
    if body.is_empty() {
        return Err(MemoryRouteError::InvalidRequest);
    }
    let value: Value =
        crate::parse_control_json(body).map_err(|_| MemoryRouteError::InvalidRequest)?;
    let object = value.as_object().ok_or(MemoryRouteError::InvalidRequest)?;
    if object.is_empty()
        || object.keys().any(|key| {
            !valid_id(key)
                || [
                    "url",
                    "path",
                    "credential",
                    "token",
                    "secret",
                    "rpc",
                    "native",
                ]
                .iter()
                .any(|forbidden| key.to_ascii_lowercase().contains(forbidden))
        })
    {
        return Err(MemoryRouteError::InvalidRequest);
    }
    for (key, value) in object {
        if (key.ends_with("_id") || key.ends_with("_ref") || key == "branch_id")
            && value.as_str().is_some_and(|value| !valid_id(value))
        {
            return Err(MemoryRouteError::InvalidRequest);
        }
        if value.as_str().is_some_and(|value| {
            value.contains("://")
                || value.starts_with('/')
                || value.contains('\\')
                || value.contains('%')
        }) {
            return Err(MemoryRouteError::InvalidRequest);
        }
        if key == "idempotency_key" && !value.as_str().is_some_and(valid_id) {
            return Err(MemoryRouteError::InvalidRequest);
        }
    }
    validate_public_value(&value).map_err(|_| MemoryRouteError::InvalidRequest)?;
    Ok(value)
}

fn owner_result(
    operation: OwnerOperation,
    revocation_epoch: u64,
    reply: crate::owner::OwnerReply,
) -> Result<Value, MemoryRouteError> {
    let receipt =
        serde_json::to_value(&reply.receipt).map_err(|_| MemoryRouteError::Unsupported)?;
    let outcome = reply.receipt.outcome;
    let value = match reply.value {
        Some(value) => value,
        None => Value::Null,
    };
    Ok(json!({
        "schema": "ascension.context-memory.owner-result.v1",
        "operation": operation.as_str(),
        "source": reply.receipt.source,
        "owner_epoch": reply.receipt.owner_epoch,
        "revocation_epoch": revocation_epoch,
        "evidence": reply.receipt.evidence,
        "outcome": outcome.as_str(),
        "effect_class": if operation.read_only() {
            "local_read_no_inference"
        } else {
            "owner_delegated"
        },
        "receipt": receipt,
        "value": value,
    }))
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> MemoryScope {
        MemoryScope {
            project_id: "project".to_owned(),
            run_id: "run".to_owned(),
            episode_id: "episode".to_owned(),
            agent_id: "agent".to_owned(),
        }
    }

    #[test]
    fn disabled_capability_is_explicit_and_search_has_no_inference() {
        let mut route = MemoryRoute::new(scope(), false);
        route.grant_search("operator");
        assert_eq!(
            route.capabilities().supported_operations,
            Vec::<String>::new()
        );
        let request = serde_json::json!({
            "schema": MEMORY_QUERY_SCHEMA,
            "scope": scope(),
            "branch_id": "branch-a",
            "query": "HP settled",
            "cutoff": 10,
            "corpus_generation": 1,
            "ranker_version": "lexical-v1",
            "limit": 8,
            "max_candidates": 64,
            "effect_class": "local_read_no_inference"
        });
        let response = route
            .handle(
                "POST",
                "/v3/memory/search",
                "operator",
                &serde_json::to_vec(&request).unwrap_or_default(),
            )
            .unwrap_or(Value::Null);
        assert_eq!(response["inference_calls"], 0);
        assert_eq!(response["coverage"], "projection_unavailable");
    }

    #[test]
    fn generate_requires_separate_review_permission() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        assert_eq!(
            route.handle("POST", "/v3/memory/generate", "searcher", b"{}"),
            Err(MemoryRouteError::PermissionDenied)
        );
    }

    #[test]
    fn enabled_facade_does_not_advertise_unattached_lanes() {
        let mut route = MemoryRoute::new(scope(), true);
        route.grant_search("searcher");
        let capabilities = route.capabilities();
        assert_eq!(capabilities.supported_operations, vec!["search"]);
        assert_eq!(capabilities.local_lexical_retrieval, "unverified");
        assert_eq!(capabilities.abstractive_adapter, "unsupported");
        assert_eq!(capabilities.per_decision_policy, "unverified");
        let status = route
            .handle("GET", "/v3/memory/status", "searcher", &[])
            .unwrap_or(Value::Null);
        assert!(status["projection_generation"].is_null());
        assert!(status["corpus_generation"].is_null());
    }
}
