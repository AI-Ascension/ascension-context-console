// SPDX-License-Identifier: MIT

//! Authenticated, bounded client projection for Phase 4 provider sessions.
//!
//! The target repository is a console, not the session authority.  This fixture route mirrors the
//! typed product namespace and records only operation metadata; native IDs, credentials, provider
//! RPC methods and turn submission are never accepted from a caller.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const SESSION_API_SCHEMA: &str = "ascension.provider-session.api-result.v1";
const MAX_BODY_BYTES: usize = 16 * 1024;
const MAX_BINDINGS: usize = 128;
const MAX_OPERATIONS: usize = 512;
const MAX_CANDIDATES: usize = 4;
const FIXTURE_EXPIRY_SECONDS: u64 = 900;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionApiError {
    BadRequest,
    Unauthorized,
    AuthNeeded,
    Forbidden,
    NotFound,
    MethodNotAllowed,
    Unsupported,
    Capacity,
    Conflict,
    Stale,
    Expired,
    Ambiguous,
    Unavailable,
    MalformedPeer,
}

impl std::fmt::Display for SessionApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BadRequest => "provider-session request is invalid",
            Self::Unauthorized => "provider-session authorization is required",
            Self::AuthNeeded => "provider-session authentication is needed",
            Self::Forbidden => "provider-session operation is forbidden",
            Self::NotFound => "provider-session resource is unavailable",
            Self::MethodNotAllowed => "provider-session method is not allowed",
            Self::Unsupported => "provider-session capability is unsupported",
            Self::Capacity => "provider-session capacity exceeded",
            Self::Conflict => "provider-session request conflicts",
            Self::Stale => "provider-session view is stale",
            Self::Expired => "provider-session authorization or preview expired",
            Self::Ambiguous => "provider-session operation outcome is ambiguous",
            Self::Unavailable => "provider-session provider state is unavailable",
            Self::MalformedPeer => "provider-session peer response is malformed",
        })
    }
}

impl std::error::Error for SessionApiError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRouteMode {
    Disabled,
    FixtureOnly,
    InspectOnly,
    Enabled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionHardeningView {
    pub tools_enabled: bool,
    pub ambient_history: bool,
    pub encrypted_state: bool,
    pub configuration_verified: bool,
    pub transform_handling: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCapabilitiesView {
    pub schema: String,
    pub profile_id: String,
    pub profile_sha256: String,
    pub native_version: String,
    pub native_binary_sha256: String,
    pub native_schema_sha256: String,
    pub evidence: String,
    pub transport: String,
    pub enabled_methods: Vec<String>,
    pub hardening: SessionHardeningView,
    pub strict_executable: bool,
    pub experimental_api: bool,
    pub unknown_methods: String,
    pub raw_rpc: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionScopeView {
    pub project_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub agent_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBindingView {
    pub schema: String,
    pub binding_id: String,
    pub scope: SessionScopeView,
    pub branch_id: String,
    pub credential_realm_ref: String,
    pub profile_sha256: String,
    pub owner_epoch: u64,
    pub session_epoch: u64,
    #[serde(skip)]
    pub run_id: String,
    pub state: String,
    pub purpose: String,
    pub native_thread_ref: String,
    pub dependency_ids: Vec<String>,
    pub continuity_sha256: String,
    pub history_coverage: String,
    pub history_epoch: u64,
    pub compaction_epoch: u64,
    #[serde(skip)]
    pub dependency_count: usize,
    pub game_dispatch_capability: bool,
    pub expires_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOperationView {
    pub schema: String,
    pub operation_id: String,
    pub scope: SessionScopeView,
    pub binding_id: String,
    pub kind: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub state: String,
    pub owner_epoch: u64,
    pub session_epoch: u64,
    pub generation_permission: bool,
    pub generation_class: bool,
    pub automatic_retry: bool,
    pub auto_resume: bool,
    pub game_effects: u64,
    pub terminal_evidence_ref: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ProviderSessionRoute {
    principal: String,
    mode: SessionRouteMode,
    capabilities: SessionCapabilitiesView,
    bindings: BTreeMap<String, SessionBindingView>,
    operations: BTreeMap<String, SessionOperationView>,
    idempotency: BTreeMap<String, IdempotencyRecord>,
    next_id: u64,
}

#[derive(Clone, Debug)]
struct IdempotencyRecord {
    request_sha256: String,
    operation_id: String,
    binding_id: String,
}

impl ProviderSessionRoute {
    #[must_use]
    pub fn fixture(principal: impl Into<String>) -> Self {
        Self {
            principal: principal.into(),
            mode: SessionRouteMode::FixtureOnly,
            capabilities: SessionCapabilitiesView {
                schema: "ascension.provider-session.capabilities.v1".to_owned(),
                profile_id: "codex-app-server-fixture-v1".to_owned(),
                profile_sha256: sha256_hex("codex-app-server-fixture-v1"),
                native_version: "fixture-peer-1".to_owned(),
                native_binary_sha256: sha256_hex("compiled-fake-native-peer"),
                native_schema_sha256: sha256_hex("codex-app-server-jsonrpc.v2"),
                evidence: "compiled_peer".to_owned(),
                transport: "owned_stdio".to_owned(),
                enabled_methods: vec![
                    "initialize".to_owned(),
                    "thread/start".to_owned(),
                    "thread/read".to_owned(),
                    "turn/start".to_owned(),
                    "turn/interrupt".to_owned(),
                    "thread/fork".to_owned(),
                    "thread/compact/start".to_owned(),
                ],
                hardening: SessionHardeningView {
                    tools_enabled: false,
                    ambient_history: false,
                    encrypted_state: false,
                    configuration_verified: true,
                    transform_handling: "detect_and_fence".to_owned(),
                },
                strict_executable: false,
                experimental_api: false,
                unknown_methods: "deny".to_owned(),
                raw_rpc: false,
            },
            bindings: BTreeMap::new(),
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            next_id: 1,
        }
    }

    #[must_use]
    pub fn disabled(principal: impl Into<String>) -> Self {
        let mut route = Self::fixture(principal);
        route.mode = SessionRouteMode::Disabled;
        route
    }

    #[must_use]
    pub fn inspect_only(principal: impl Into<String>) -> Self {
        let mut route = Self::fixture(principal);
        route.mode = SessionRouteMode::InspectOnly;
        route
    }

    #[must_use]
    pub fn mode(&self) -> SessionRouteMode {
        self.mode.clone()
    }

    pub fn handle(
        &mut self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if principal != self.principal || principal.is_empty() {
            return Err(SessionApiError::Unauthorized);
        }
        if body.len() > MAX_BODY_BYTES
            || path.contains("..")
            || path.contains('\\')
            || path.contains('%')
        {
            return Err(SessionApiError::BadRequest);
        }
        if method == "GET" && !body.is_empty() {
            return Err(SessionApiError::BadRequest);
        }
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments.len() < 4
            || segments[0] != "v1"
            || segments[1] != "runs"
            || !matches!(
                segments[3],
                "provider-sessions" | "provider-session-operations" | "provider-session-events"
            )
        {
            return Err(SessionApiError::NotFound);
        }
        let run_id = segments[2];
        if !valid_id(run_id) {
            return Err(SessionApiError::BadRequest);
        }
        if self.mode == SessionRouteMode::Disabled {
            return Err(SessionApiError::Unsupported);
        }
        if self.mode == SessionRouteMode::InspectOnly && method != "GET" {
            return Err(SessionApiError::Unsupported);
        }
        if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "capabilities"
        {
            return if method == "GET" {
                Ok(self.envelope(
                    "capabilities",
                    serde_json::to_value(&self.capabilities)
                        .map_err(|_| SessionApiError::BadRequest)?,
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 4 && segments[3] == "provider-sessions" {
            return if method == "GET" {
                let bindings = self
                    .bindings
                    .values()
                    .filter(|binding| binding.scope.run_id == run_id)
                    .collect::<Vec<_>>();
                Ok(self.envelope(
                    "list",
                    json!({"run_id":run_id,"bindings":bindings,"next_cursor":null}),
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 5 && segments[3] == "provider-sessions" && segments[4] == "candidates"
        {
            return self.mutate_candidate(method, run_id, body);
        }
        if segments.len() == 4 && segments[3] == "provider-session-events" {
            return if method == "GET" {
                Ok(self.envelope(
                    "events",
                    json!({"run_id":run_id,"events":[],"next_cursor":null}),
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 5 && segments[3] == "provider-sessions" && segments[4] == "events" {
            return if method == "GET" {
                Ok(self.envelope(
                    "events",
                    json!({"run_id":run_id,"events":[],"next_cursor":null}),
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments[3] == "provider-session-operations" {
            if segments.len() != 5 || method != "GET" {
                return Err(SessionApiError::MethodNotAllowed);
            }
            let operation = self
                .operations
                .get(segments.get(4).copied().unwrap_or_default())
                .ok_or(SessionApiError::NotFound)?;
            if operation.scope.run_id != run_id {
                return Err(SessionApiError::NotFound);
            }
            return Ok(self.envelope(
                "operation",
                serde_json::to_value(operation).map_err(|_| SessionApiError::BadRequest)?,
            ));
        }
        if segments[3] != "provider-sessions" {
            return Err(SessionApiError::NotFound);
        }
        if segments.len() == 5 {
            let binding = self
                .bindings
                .get(segments[4])
                .ok_or(SessionApiError::NotFound)?;
            if binding.scope.run_id != run_id {
                return Err(SessionApiError::NotFound);
            }
            return if method == "GET" {
                Ok(self.envelope(
                    "binding",
                    serde_json::to_value(binding).map_err(|_| SessionApiError::BadRequest)?,
                ))
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        let binding_id = segments[4];
        let binding = self
            .bindings
            .get(binding_id)
            .ok_or(SessionApiError::NotFound)?
            .clone();
        if binding.scope.run_id != run_id {
            return Err(SessionApiError::NotFound);
        }
        if segments.len() == 6 && segments[5] == "history" {
            return if method == "GET" {
                Ok(self.envelope("history", json!({"schema":"ascension.provider-session.history.v1","view_id":format!("history-view-{binding_id}"),"binding_id":binding_id,"scope":binding.scope,"history_epoch":binding.history_epoch,"watermark":0,"coverage":binding.history_coverage,"effective_context_coverage":"unknown","items":[],"known_total_items":0,"next_cursor":null,"read_started_turn":false,"expires_at":binding.expires_at})))
            } else if method == "POST" {
                self.accept_operation(binding_id, "refresh", false, body)
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 6
            && matches!(
                segments[5],
                "fork-plans" | "compaction-plans" | "prepared-bindings"
            )
        {
            return if method == "POST" {
                self.accept_plan(binding_id, segments[5], body)
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        if segments.len() == 6 {
            let operation = match segments[5] {
                "reconnect" => "reconnect",
                "history-refresh" => "refresh",
                "fork-jobs" => "fork",
                "compaction-jobs" => "compact",
                "retire" => "retire",
                "cleanup" => "cleanup",
                _ => return Err(SessionApiError::NotFound),
            };
            return if method == "POST" {
                self.accept_operation(binding_id, operation, operation == "compact", body)
            } else {
                Err(SessionApiError::MethodNotAllowed)
            };
        }
        Err(SessionApiError::NotFound)
    }

    fn mutate_candidate(
        &mut self,
        method: &str,
        run_id: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if method != "POST" {
            return Err(SessionApiError::MethodNotAllowed);
        }
        if body.len() > MAX_BODY_BYTES {
            return Err(SessionApiError::Capacity);
        }
        let (value, idempotency_key) = parse_command(
            body,
            &[
                "idempotency_key",
                "expected_control_generation",
                "approved_policy_ref",
                "profile_ref",
                "purpose",
            ],
            &[
                "idempotency_key",
                "expected_control_generation",
                "approved_policy_ref",
                "profile_ref",
                "purpose",
            ],
        )?;
        let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
        require_u64(object, "expected_control_generation")?;
        require_id(object, "approved_policy_ref")?;
        require_id(object, "profile_ref")?;
        match object.get("purpose").and_then(Value::as_str) {
            Some("executable_candidate" | "evaluation") => {}
            _ => return Err(SessionApiError::BadRequest),
        }
        let request_sha256 = canonical_digest(&value);
        let dedupe_key = format!("candidate:{run_id}:{idempotency_key}");
        if let Some(existing) = self.idempotency.get(&dedupe_key) {
            if existing.request_sha256 != request_sha256 {
                return Err(SessionApiError::Conflict);
            }
            return Ok(self.accepted_operation(existing));
        }
        if self.bindings.len() >= MAX_BINDINGS {
            return Err(SessionApiError::Capacity);
        }
        if self
            .bindings
            .values()
            .filter(|binding| binding.scope.run_id == run_id)
            .filter(|binding| binding.state == "candidate" || binding.purpose == "evaluation")
            .count()
            >= MAX_CANDIDATES
        {
            return Err(SessionApiError::Capacity);
        }
        let binding_id = format!("binding-{}", self.next_id);
        let operation_id = format!("operation-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let purpose = if object
            .get("purpose")
            .and_then(Value::as_str)
            .is_some_and(|purpose| purpose == "evaluation")
        {
            "evaluation"
        } else {
            "executable"
        };
        let scope = scope_for_run(run_id);
        self.bindings.insert(
            binding_id.clone(),
            SessionBindingView {
                schema: "ascension.provider-session.binding.v1".to_owned(),
                binding_id: binding_id.clone(),
                scope: scope.clone(),
                branch_id: "branch-fixture".to_owned(),
                credential_realm_ref: "fixture-realm".to_owned(),
                profile_sha256: sha256_hex("codex-app-server-fixture-v1"),
                owner_epoch: 1,
                session_epoch: 1,
                run_id: run_id.to_owned(),
                state: "candidate".to_owned(),
                purpose: purpose.to_owned(),
                native_thread_ref: format!("pending-{binding_id}"),
                dependency_ids: Vec::new(),
                continuity_sha256: sha256_hex("empty-provider-history"),
                history_coverage: "unknown".to_owned(),
                history_epoch: 0,
                compaction_epoch: 0,
                dependency_count: 0,
                game_dispatch_capability: false,
                expires_at: fixture_expiry(),
            },
        );
        self.operations.insert(
            operation_id.clone(),
            SessionOperationView {
                schema: "ascension.provider-session.operation.v1".to_owned(),
                operation_id: operation_id.clone(),
                scope,
                binding_id: binding_id.clone(),
                kind: "create_candidate".to_owned(),
                idempotency_key: idempotency_key.clone(),
                request_sha256: request_sha256.clone(),
                state: "intent_persisted".to_owned(),
                owner_epoch: 1,
                session_epoch: 1,
                generation_permission: false,
                generation_class: false,
                automatic_retry: false,
                auto_resume: false,
                game_effects: 0,
                terminal_evidence_ref: None,
            },
        );
        self.idempotency.insert(
            dedupe_key,
            IdempotencyRecord {
                request_sha256,
                operation_id: operation_id.clone(),
                binding_id: binding_id.clone(),
            },
        );
        Ok(self.envelope("accepted", json!({"operation_id":operation_id,"binding_id":binding_id,"status":"intent_persisted","native_calls":0,"game_effects":0})))
    }

    fn accept_plan(
        &mut self,
        binding_id: &str,
        kind: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if body.len() > MAX_BODY_BYTES {
            return Err(SessionApiError::Capacity);
        }
        let (value, idempotency_key) = match kind {
            "fork-plans" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "cutoff_turn_ref",
                    "operation",
                    "purpose",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "cutoff_turn_ref",
                    "operation",
                    "purpose",
                ],
            )?,
            "compaction-plans" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "requested_policy",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "requested_policy",
                ],
            )?,
            "prepared-bindings" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "phase2_draft_ref",
                    "phase3_selection_ref",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                    "phase2_draft_ref",
                    "phase3_selection_ref",
                ],
            )?,
            _ => return Err(SessionApiError::Unsupported),
        };
        let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
        for field in [
            "expected_control_generation",
            "expected_session_epoch",
            "expected_history_epoch",
        ] {
            require_u64(object, field)?;
        }
        match kind {
            "fork-plans" => {
                require_id(object, "cutoff_turn_ref")?;
                if !matches!(
                    object.get("operation").and_then(Value::as_str),
                    Some("native_fork" | "clean_rehydration")
                ) || object.get("purpose").and_then(Value::as_str) != Some("evaluation")
                {
                    return Err(SessionApiError::BadRequest);
                }
            }
            "compaction-plans" => {
                if !matches!(
                    object.get("requested_policy").and_then(Value::as_str),
                    Some("strict_reviewed" | "observed_persistent")
                ) {
                    return Err(SessionApiError::BadRequest);
                }
            }
            "prepared-bindings" => {
                require_id(object, "phase2_draft_ref")?;
                require_id(object, "phase3_selection_ref")?;
            }
            _ => return Err(SessionApiError::Unsupported),
        }
        let request_sha256 = canonical_digest(&value);
        let dedupe_key = format!("plan:{kind}:{binding_id}:{idempotency_key}");
        if let Some(existing) = self.idempotency.get(&dedupe_key) {
            if existing.request_sha256 != request_sha256 {
                return Err(SessionApiError::Conflict);
            }
            return Ok(self.accepted_plan(existing, kind));
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        if self.idempotency.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        let plan_id = format!("plan-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        if matches!(kind, "fork-plans" | "compaction-plans") {
            let (scope, session_epoch) = self
                .bindings
                .get(binding_id)
                .map(|binding| (binding.scope.clone(), binding.session_epoch))
                .ok_or(SessionApiError::NotFound)?;
            let operation_kind = if kind == "fork-plans" {
                "fork"
            } else {
                "compact"
            };
            self.operations.insert(
                plan_id.clone(),
                SessionOperationView {
                    schema: "ascension.provider-session.operation.v1".to_owned(),
                    operation_id: plan_id.clone(),
                    scope,
                    binding_id: binding_id.to_owned(),
                    kind: operation_kind.to_owned(),
                    idempotency_key: idempotency_key.clone(),
                    request_sha256: request_sha256.clone(),
                    state: "planned".to_owned(),
                    owner_epoch: 1,
                    session_epoch,
                    generation_permission: kind == "compaction-plans",
                    generation_class: kind == "compaction-plans",
                    automatic_retry: false,
                    auto_resume: false,
                    game_effects: 0,
                    terminal_evidence_ref: None,
                },
            );
        }
        self.idempotency.insert(
            dedupe_key,
            IdempotencyRecord {
                request_sha256,
                operation_id: plan_id.clone(),
                binding_id: binding_id.to_owned(),
            },
        );
        Ok(self.envelope(
            "plan",
            json!({
                "plan_id": plan_id,
                "binding_id": binding_id,
                "kind": kind,
                "status": "planned",
                "inference_calls": 0,
                "game_effects": 0
            }),
        ))
    }

    fn accept_operation(
        &mut self,
        binding_id: &str,
        kind: &str,
        generation_permission: bool,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if body.len() > MAX_BODY_BYTES {
            return Err(SessionApiError::Capacity);
        }
        let (value, idempotency_key) = match kind {
            "refresh" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "expected_history_epoch",
                ],
            )?,
            "reconnect" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                ],
            )?,
            "fork" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_fork_plan_ref",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_fork_plan_ref",
                ],
            )?,
            "compact" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_compaction_plan_ref",
                    "spend_authorization_ref",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "approved_compaction_plan_ref",
                    "spend_authorization_ref",
                ],
            )?,
            "retire" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "reason",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "expected_session_epoch",
                    "reason",
                ],
            )?,
            "cleanup" => parse_command(
                body,
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "retirement_ref",
                    "erase_authorization_ref",
                    "expected_session_epoch",
                ],
                &[
                    "idempotency_key",
                    "expected_control_generation",
                    "retirement_ref",
                    "erase_authorization_ref",
                    "expected_session_epoch",
                ],
            )?,
            _ => return Err(SessionApiError::Unsupported),
        };
        let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
        for field in [
            "expected_control_generation",
            "expected_session_epoch",
            "expected_history_epoch",
        ] {
            if object.contains_key(field) {
                require_u64(object, field)?;
            }
        }
        for field in [
            "approved_fork_plan_ref",
            "approved_compaction_plan_ref",
            "spend_authorization_ref",
            "retirement_ref",
            "erase_authorization_ref",
        ] {
            if object.contains_key(field) {
                require_id(object, field)?;
            }
        }
        if kind == "retire"
            && !matches!(
                object.get("reason").and_then(Value::as_str),
                Some(
                    "operator_request"
                        | "source_removed"
                        | "continuity_lost"
                        | "scope_ended"
                        | "capacity_rotation"
                )
            )
        {
            return Err(SessionApiError::BadRequest);
        }
        if kind == "compact" && !object.contains_key("spend_authorization_ref") {
            return Err(SessionApiError::BadRequest);
        }
        let request_sha256 = canonical_digest(&value);
        let dedupe_key = format!("{kind}:{binding_id}:{idempotency_key}");
        if let Some(existing) = self.idempotency.get(&dedupe_key) {
            if existing.request_sha256 != request_sha256 {
                return Err(SessionApiError::Conflict);
            }
            return Ok(self.accepted_operation(existing));
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        let scope = self
            .bindings
            .get(binding_id)
            .map(|binding| binding.scope.clone())
            .ok_or(SessionApiError::NotFound)?;
        let session_epoch = self
            .bindings
            .get(binding_id)
            .map_or(1, |binding| binding.session_epoch);
        let operation_id = format!("operation-{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.operations.insert(
            operation_id.clone(),
            SessionOperationView {
                schema: "ascension.provider-session.operation.v1".to_owned(),
                operation_id: operation_id.clone(),
                scope,
                binding_id: binding_id.to_owned(),
                kind: kind.to_owned(),
                idempotency_key: idempotency_key.clone(),
                request_sha256: request_sha256.clone(),
                state: "intent_persisted".to_owned(),
                owner_epoch: 1,
                session_epoch,
                generation_permission,
                generation_class: generation_permission,
                automatic_retry: false,
                auto_resume: false,
                game_effects: 0,
                terminal_evidence_ref: None,
            },
        );
        self.idempotency.insert(
            dedupe_key,
            IdempotencyRecord {
                request_sha256,
                operation_id: operation_id.clone(),
                binding_id: binding_id.to_owned(),
            },
        );
        Ok(self.envelope("accepted", json!({"operation_id":operation_id,"binding_id":binding_id,"status":"intent_persisted","native_calls":0,"game_effects":0})))
    }

    fn accepted_operation(&self, record: &IdempotencyRecord) -> Value {
        self.envelope(
            "accepted",
            json!({
                "operation_id": record.operation_id,
                "binding_id": record.binding_id,
                "status": "intent_persisted",
                "native_calls": 0,
                "game_effects": 0
            }),
        )
    }

    fn accepted_plan(&self, record: &IdempotencyRecord, kind: &str) -> Value {
        self.envelope(
            "plan",
            json!({
                "plan_id": record.operation_id,
                "binding_id": record.binding_id,
                "kind": kind,
                "status": "planned",
                "inference_calls": 0,
                "game_effects": 0
            }),
        )
    }

    fn envelope(&self, operation: &str, value: Value) -> Value {
        json!({"schema":SESSION_API_SCHEMA,"operation":operation,"value":value,"effect_class":"local_metadata_only","inference_calls":0,"game_effects":0})
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

fn scope_for_run(run_id: &str) -> SessionScopeView {
    SessionScopeView {
        project_id: "project-fixture".to_owned(),
        run_id: run_id.to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    }
}

fn fixture_expiry() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    crate::control::format_time(now.saturating_add(FIXTURE_EXPIRY_SECONDS))
}

fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    use sha2::{Digest as _, Sha256};
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn parse_command(
    body: &[u8],
    required: &[&str],
    allowed: &[&str],
) -> Result<(Value, String), SessionApiError> {
    if body.is_empty() {
        return Err(SessionApiError::BadRequest);
    }
    let value: Value = crate::parse_control_json(body).map_err(|_| SessionApiError::BadRequest)?;
    let object = value.as_object().ok_or(SessionApiError::BadRequest)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(SessionApiError::BadRequest);
    }
    if required.iter().any(|key| !object.contains_key(*key)) {
        return Err(SessionApiError::BadRequest);
    }
    let key = object
        .get("idempotency_key")
        .and_then(Value::as_str)
        .filter(|key| valid_id(key))
        .ok_or(SessionApiError::BadRequest)?
        .to_owned();
    Ok((value, key))
}

fn require_id(object: &serde_json::Map<String, Value>, field: &str) -> Result<(), SessionApiError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| valid_id(value))
        .map(|_| ())
        .ok_or(SessionApiError::BadRequest)
}

fn require_u64(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), SessionApiError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .map(|_| ())
        .ok_or(SessionApiError::BadRequest)
}

fn canonical_digest(value: &Value) -> String {
    let canonical = canonical_value(value);
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    sha256_hex(bytes)
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
