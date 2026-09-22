// SPDX-License-Identifier: MIT

use super::capabilities::{fixture_session_binding, fixture_session_effective_limits};
use super::support::*;
use super::*;

impl ProviderSessionRoute {
    #[must_use]
    pub fn fixture(principal: impl Into<String>) -> Self {
        let mut route = Self {
            principal: principal.into(),
            mode: SessionRouteMode::FixtureOnly,
            capabilities: SessionCapabilitiesView {
                schema: SESSION_CAPABILITIES_SCHEMA.to_owned(),
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
                    // Widened from `const true` to `boolean` by the producer. The console keeps its
                    // independent private-retention guard (accepted policy approval plus
                    // authenticated encryption) regardless of this advertised value.
                    encrypted_state: false,
                    configuration_verified: true,
                    transform_handling: "detect_and_fence".to_owned(),
                },
                effective_limits: fixture_session_effective_limits(),
                binding: fixture_session_binding(),
                strict_executable: false,
                experimental_api: false,
                unknown_methods: "deny".to_owned(),
                raw_rpc: false,
            },
            bindings: BTreeMap::new(),
            operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            next_id: 1,
            owner: None,
            attached_scope: None,
            attached_bindings: BTreeMap::new(),
            attached_operations: BTreeMap::new(),
            capability_version: crate::CapabilityVersion::V3,
        };
        // Serialization failure leaves an invalid empty digest; the served route fails closed.
        if let Ok(digest) = route.capabilities.descriptor_digest() {
            route.capabilities.binding.descriptor_sha256 = digest;
        }
        route
    }

    /// Builds the explicitly attached composition. The route owns no native session state in this
    /// mode; all session, history and compaction decisions are delegated to `composition`.
    #[must_use]
    pub fn attached(principal: impl Into<String>, composition: HarnessOwnerComposition) -> Self {
        let mut route = Self::fixture(principal);
        route.mode = SessionRouteMode::Enabled;
        route.owner = Some(composition);
        route.bindings.clear();
        route.operations.clear();
        route.idempotency.clear();
        route
    }

    /// Attached constructor with an explicit owner scope. This is the production composition
    /// entry point; the compatibility constructor above uses the historical fixture scope.
    #[must_use]
    pub fn attached_with_scope(
        principal: impl Into<String>,
        scope: SessionScopeView,
        composition: HarnessOwnerComposition,
    ) -> Self {
        let mut route = Self::attached(principal, composition);
        route.attached_scope = Some(scope);
        route
    }

    #[must_use]
    pub fn with_owner(
        principal: impl Into<String>,
        owner: std::sync::Arc<dyn crate::owner::HarnessOwner>,
        grants: OwnerGrantBook,
    ) -> Self {
        Self::attached(principal, HarnessOwnerComposition::new(owner, grants))
    }

    #[must_use]
    pub fn with_owner_scope(
        principal: impl Into<String>,
        scope: SessionScopeView,
        owner: std::sync::Arc<dyn crate::owner::HarnessOwner>,
        grants: OwnerGrantBook,
    ) -> Self {
        Self::attached_with_scope(
            principal,
            scope,
            HarnessOwnerComposition::new(owner, grants),
        )
    }

    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.owner.is_some()
    }

    pub fn revoke_grant(&self, grant_id: &str) -> Result<(), crate::owner::OwnerAuthError> {
        self.owner
            .as_ref()
            .ok_or(crate::owner::OwnerAuthError::GrantBookUnavailable)?
            .revoke(grant_id)
    }

    /// Registers a binding returned by a harness owner so later path references can be checked
    /// locally before forwarding. This is useful when a composition is restored from a durable
    /// owner snapshot.
    pub fn register_attached_binding(
        &mut self,
        binding_id: impl Into<String>,
        scope: SessionScopeView,
    ) -> Result<(), SessionApiError> {
        let binding_id = binding_id.into();
        if !valid_id(&binding_id) {
            return Err(SessionApiError::BadRequest);
        }
        let expected_scope = match &self.attached_scope {
            Some(scope) => scope.clone(),
            None => scope_for_run(&scope.run_id),
        };
        if expected_scope != scope {
            return Err(SessionApiError::NotFound);
        }
        if !self.attached_bindings.contains_key(&binding_id)
            && self.attached_bindings.len() >= MAX_BINDINGS
        {
            return Err(SessionApiError::Capacity);
        }
        self.attached_bindings.insert(binding_id, scope);
        Ok(())
    }

    /// Registers an owner operation identity restored from the owner's durable receipt index.
    /// Foreign identities are rejected before any operation lookup is forwarded.
    pub fn register_attached_operation(
        &mut self,
        operation_id: impl Into<String>,
        scope: SessionScopeView,
    ) -> Result<(), SessionApiError> {
        let operation_id = operation_id.into();
        if !valid_id(&operation_id) {
            return Err(SessionApiError::BadRequest);
        }
        let expected_scope = match &self.attached_scope {
            Some(scope) => scope.clone(),
            None => scope_for_run(&scope.run_id),
        };
        if expected_scope != scope {
            return Err(SessionApiError::NotFound);
        }
        if !self.attached_operations.contains_key(&operation_id)
            && self.attached_operations.len() >= MAX_OPERATIONS
        {
            return Err(SessionApiError::Capacity);
        }
        self.attached_operations.insert(operation_id, scope);
        Ok(())
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

    /// Select an explicit legacy advertisement for rollback; v1 discloses no executable limits.
    #[must_use]
    pub fn with_capability_version(mut self, version: crate::CapabilityVersion) -> Self {
        self.capability_version = version;
        self
    }

    #[must_use]
    pub fn consumer_pin(&self) -> crate::effective_limits::ConsumerPin {
        crate::effective_limits::console_consumer_pin_for(self.capability_version)
    }

    /// The local v3 descriptor. Attached routes obtain and authenticate an actual owner reply.
    #[must_use]
    pub fn capabilities(&self) -> SessionCapabilitiesView {
        self.capabilities.clone()
    }

    /// The local synthetic v3 descriptor, validated before the served route presents it.
    #[must_use]
    pub fn target_capabilities_v3(&self) -> &SessionCapabilitiesView {
        &self.capabilities
    }

    /// Derivation of the trusted effective-limit record from the `v3` target capability descriptor.
    #[must_use]
    pub fn effective_limit_record(&self) -> EffectiveLimitRecord {
        self.capabilities.effective_limit_record()
    }

    /// Admit a session policy value against the advertised executable ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when disabled, absent, or above the executable ceiling.
    pub fn admit_policy_value(&self, field: &str, requested: u64) -> Result<(), UnavailableReason> {
        self.capabilities.admit_policy_value(field, requested)
    }

    /// Admit a value only after authenticating the published record against the advertised
    /// descriptor's derivation.
    ///
    /// # Errors
    ///
    /// Returns [`UnavailableReason`] when the record is stale, tampered, targets another profile,
    /// or the value exceeds the executable ceiling.
    pub fn admit_authorized_record(
        &self,
        record: &EffectiveLimitRecord,
        field: &str,
        requested: u64,
    ) -> Result<(), UnavailableReason> {
        self.capabilities
            .admit_authorized_record(record, field, requested)
    }

    pub fn handle(
        &mut self,
        method: &str,
        path: &str,
        principal: &str,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if self.owner.is_some() {
            // The compatibility signature cannot carry Host, Origin or CSRF proofs. Attached
            // routes therefore require the explicit `handle_with_context` entry point.
            return Err(SessionApiError::Unauthorized);
        }
        self.handle_local(method, path, principal, body)
    }

    /// Handles an attached request after validating the complete origin/Host/CSRF and grant
    /// context. The old fixture route remains available through [`Self::fixture`].
    pub fn handle_with_context(
        &mut self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
        if self.owner.is_some() {
            self.handle_attached(method, path, context, body)
        } else {
            self.handle_local(method, path, &context.principal, body)
        }
    }

    pub(super) fn handle_local(
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
                self.capabilities
                    .validate_descriptor()
                    .map_err(SessionApiError::EffectiveLimit)?;
                let value = match self.capability_version {
                    crate::CapabilityVersion::V3 => serde_json::to_value(self.capabilities()),
                    crate::CapabilityVersion::V1 => serde_json::to_value(self.capabilities.to_v1()),
                }
                .map_err(|_| SessionApiError::BadRequest)?;
                Ok(self.envelope("capabilities", value))
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
                let operations = self
                    .operations
                    .values()
                    .filter(|operation| operation.scope.run_id == run_id)
                    .collect::<Vec<_>>();
                Ok(self.envelope(
                    "list",
                    json!({"run_id":run_id,"bindings":bindings,"operations":operations,"next_cursor":null}),
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
                .get(segments.get(4).copied().ok_or(SessionApiError::NotFound)?)
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
}
