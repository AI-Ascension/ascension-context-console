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
}

impl std::fmt::Display for MemoryRouteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::MethodNotAllowed => "memory route method is not allowed",
            Self::BodyTooLarge => "memory route body exceeds its bound",
            Self::InvalidRequest => "memory route request is invalid",
            Self::PermissionDenied => "memory route permission is denied",
            Self::Unsupported => "memory route operation is unsupported",
        })
    }
}

impl std::error::Error for MemoryRouteError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryRoute {
    scope: MemoryScope,
    enabled: bool,
    search_principals: Vec<String>,
    review_principals: Vec<String>,
}

impl MemoryRoute {
    pub fn new(scope: MemoryScope, enabled: bool) -> Self {
        Self {
            scope,
            enabled,
            search_principals: Vec::new(),
            review_principals: Vec::new(),
        }
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
