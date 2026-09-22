// SPDX-License-Identifier: MIT

use super::support::*;
use super::*;

impl ProviderSessionRoute {
    pub(super) fn handle_attached(
        &mut self,
        method: &str,
        path: &str,
        context: &OwnerRequestContext,
        body: &[u8],
    ) -> Result<Value, SessionApiError> {
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
        let scope_view = match &self.attached_scope {
            Some(scope) if scope.run_id != run_id => return Err(SessionApiError::NotFound),
            Some(scope) => scope.clone(),
            None => scope_for_run(run_id),
        };
        let owner_scope = owner_scope(
            &scope_view.project_id,
            &scope_view.run_id,
            &scope_view.episode_id,
            &scope_view.agent_id,
        );
        let (operation, reference, write) = if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "capabilities"
        {
            (OwnerOperation::SessionCapabilities, None, false)
        } else if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "status"
        {
            (OwnerOperation::SessionStatus, None, false)
        } else if segments.len() == 4 && segments[3] == "provider-sessions" {
            (OwnerOperation::SessionList, None, false)
        } else if segments.len() == 5
            && segments[3] == "provider-sessions"
            && segments[4] == "candidates"
        {
            (OwnerOperation::SessionCandidate, None, true)
        } else if (segments.len() == 4 && segments[3] == "provider-session-events")
            || (segments.len() == 5
                && segments[3] == "provider-sessions"
                && segments[4] == "events")
        {
            (OwnerOperation::SessionEvents, None, false)
        } else if segments.len() == 5 && segments[3] == "provider-session-operations" {
            let operation_id = segments[4];
            if !valid_id(operation_id)
                || self
                    .attached_operations
                    .get(operation_id)
                    .is_none_or(|scope| scope.run_id != run_id)
            {
                return Err(SessionApiError::NotFound);
            }
            (
                OwnerOperation::SessionOperation,
                Some(operation_id.to_owned()),
                false,
            )
        } else if segments[3] == "provider-sessions" && segments.len() >= 5 {
            let binding_id = segments[4];
            let Some(binding_scope) = self.attached_bindings.get(binding_id) else {
                return Err(SessionApiError::NotFound);
            };
            if binding_scope.run_id != run_id || !valid_id(binding_id) {
                return Err(SessionApiError::NotFound);
            }
            if segments.len() == 5 {
                if method != "GET" {
                    return Err(SessionApiError::MethodNotAllowed);
                }
                (
                    OwnerOperation::SessionBinding,
                    Some(binding_id.to_owned()),
                    false,
                )
            } else if segments.len() == 6 && segments[5] == "history" {
                if method == "GET" {
                    (
                        OwnerOperation::SessionHistory,
                        Some(binding_id.to_owned()),
                        false,
                    )
                } else if method == "POST" {
                    (
                        OwnerOperation::SessionHistoryRefresh,
                        Some(binding_id.to_owned()),
                        true,
                    )
                } else {
                    return Err(SessionApiError::MethodNotAllowed);
                }
            } else if segments.len() == 6 && segments[5] == "fork-plans" {
                (
                    OwnerOperation::SessionForkPlan,
                    Some(binding_id.to_owned()),
                    true,
                )
            } else if segments.len() == 6 && segments[5] == "compaction-plans" {
                (
                    OwnerOperation::SessionCompactionPlan,
                    Some(binding_id.to_owned()),
                    true,
                )
            } else if segments.len() == 6 && segments[5] == "prepared-bindings" {
                (
                    OwnerOperation::SessionPreparedBinding,
                    Some(binding_id.to_owned()),
                    true,
                )
            } else if segments.len() == 6 {
                let operation = match segments[5] {
                    "reconnect" => OwnerOperation::SessionReconnect,
                    "history-refresh" => OwnerOperation::SessionHistoryRefresh,
                    "fork-jobs" => OwnerOperation::SessionFork,
                    "compaction-jobs" => OwnerOperation::SessionCompaction,
                    "retire" => OwnerOperation::SessionRetire,
                    "cleanup" => OwnerOperation::SessionCleanup,
                    _ => return Err(SessionApiError::NotFound),
                };
                (operation, Some(binding_id.to_owned()), true)
            } else {
                return Err(SessionApiError::NotFound);
            }
        } else {
            return Err(SessionApiError::NotFound);
        };

        let payload = owner_session_payload(operation, method, body)?;
        self.delegate_attached(operation, owner_scope, reference, payload, context, write)
    }

    pub(super) fn delegate_attached(
        &mut self,
        operation: OwnerOperation,
        scope: crate::owner::OwnerScope,
        reference: Option<String>,
        payload: Value,
        context: &OwnerRequestContext,
        write: bool,
    ) -> Result<Value, SessionApiError> {
        let composition = self.owner.as_ref().ok_or(SessionApiError::Unavailable)?;
        let grant = composition
            .authorize(context, operation.grant_class(), &scope, write)
            .map_err(|_| SessionApiError::Forbidden)?;
        self.ensure_attached_capacity(operation)?;
        let revocation_epoch = grant.revocation_epoch;
        let call = owner_call(operation, scope.clone(), reference.clone(), payload, grant)
            .map_err(|_| SessionApiError::BadRequest)?;
        let owner = composition.owner();
        let mut reply = match owner.call(call) {
            Ok(reply) => reply,
            Err(OwnerError::LostReply { receipt_id }) => {
                if !valid_id(&receipt_id) {
                    return Err(SessionApiError::Unavailable);
                }
                let lookup = crate::owner::OwnerReceiptLookup {
                    operation,
                    scope: scope.clone(),
                    reference,
                    receipt_id: receipt_id.clone(),
                };
                match owner.lookup_receipt(lookup) {
                    Ok(reply) => {
                        if reply.receipt.receipt_id != receipt_id {
                            return Err(SessionApiError::MalformedPeer);
                        }
                        reply
                    }
                    Err(OwnerError::UnknownReceipt | OwnerError::Unavailable) => {
                        crate::owner::OwnerReply::unknown(
                            receipt_id,
                            format!("unknown-{}", operation.as_str().replace('.', "-")),
                            operation,
                            "harness",
                            0,
                            "receipt_lookup_unavailable",
                        )
                        .map_err(|_| SessionApiError::MalformedPeer)?
                    }
                    Err(OwnerError::Unsupported) => crate::owner::OwnerReply::unsupported(
                        receipt_id.clone(),
                        "unsupported",
                        operation,
                        "harness",
                        0,
                        "owner_unsupported",
                    )
                    .map_err(|_| SessionApiError::MalformedPeer)?,
                    Err(OwnerError::LostReply { .. } | OwnerError::MalformedResponse) => {
                        return Err(SessionApiError::Unavailable);
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
            .map_err(|_| SessionApiError::MalformedPeer)?,
            Err(OwnerError::Unavailable | OwnerError::UnknownReceipt) => {
                return Err(SessionApiError::Unavailable);
            }
            Err(OwnerError::MalformedResponse) => return Err(SessionApiError::MalformedPeer),
        };
        reply
            .validate_for(operation)
            .map_err(|_| SessionApiError::MalformedPeer)?;
        composition
            .capability_trust
            .present(operation, &scope, &mut reply, self.capability_version)
            .map_err(SessionApiError::EffectiveLimit)?;
        self.record_attached_references(&scope, reply.value.as_ref())?;
        owner_session_result(operation, revocation_epoch, reply)
    }

    pub(super) fn ensure_attached_capacity(
        &self,
        operation: OwnerOperation,
    ) -> Result<(), SessionApiError> {
        let creates_binding = matches!(
            operation,
            OwnerOperation::SessionCandidate | OwnerOperation::SessionPreparedBinding
        );
        let creates_operation = matches!(
            operation,
            OwnerOperation::SessionCandidate
                | OwnerOperation::SessionHistoryRefresh
                | OwnerOperation::SessionReconnect
                | OwnerOperation::SessionForkPlan
                | OwnerOperation::SessionCompactionPlan
                | OwnerOperation::SessionPreparedBinding
                | OwnerOperation::SessionFork
                | OwnerOperation::SessionCompaction
                | OwnerOperation::SessionRetire
                | OwnerOperation::SessionCleanup
        );
        if creates_binding && self.attached_bindings.len() >= MAX_BINDINGS {
            return Err(SessionApiError::Capacity);
        }
        if creates_operation && self.attached_operations.len() >= MAX_OPERATIONS {
            return Err(SessionApiError::Capacity);
        }
        Ok(())
    }

    pub(super) fn record_attached_references(
        &mut self,
        scope: &crate::owner::OwnerScope,
        value: Option<&Value>,
    ) -> Result<(), SessionApiError> {
        let Some(object) = value.and_then(Value::as_object) else {
            return Ok(());
        };
        let binding_id = object
            .get("binding_id")
            .and_then(Value::as_str)
            .filter(|value| valid_id(value));
        if binding_id.is_some_and(|binding_id| {
            !self.attached_bindings.contains_key(binding_id)
                && self.attached_bindings.len() >= MAX_BINDINGS
        }) {
            return Err(SessionApiError::Capacity);
        }

        let mut new_operation_ids = Vec::new();
        for key in ["operation_id", "plan_id"] {
            if let Some(operation_id) = object
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| valid_id(value))
                && !self.attached_operations.contains_key(operation_id)
                && !new_operation_ids.iter().any(|known| known == operation_id)
            {
                new_operation_ids.push(operation_id.to_owned());
            }
        }
        if self
            .attached_operations
            .len()
            .saturating_add(new_operation_ids.len())
            > MAX_OPERATIONS
        {
            return Err(SessionApiError::Capacity);
        }

        if let Some(binding_id) = binding_id {
            self.attached_bindings.insert(
                binding_id.to_owned(),
                SessionScopeView {
                    project_id: scope.project_id.clone(),
                    run_id: scope.run_id.clone(),
                    episode_id: scope.episode_id.clone(),
                    agent_id: scope.agent_id.clone(),
                },
            );
        }
        for operation_id in new_operation_ids {
            self.attached_operations.insert(
                operation_id,
                SessionScopeView {
                    project_id: scope.project_id.clone(),
                    run_id: scope.run_id.clone(),
                    episode_id: scope.episode_id.clone(),
                    agent_id: scope.agent_id.clone(),
                },
            );
        }
        Ok(())
    }

    pub(super) fn mutate_candidate(
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

    pub(super) fn accept_plan(
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

    pub(super) fn accept_operation(
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

    pub(super) fn accepted_operation(&self, record: &IdempotencyRecord) -> Value {
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

    pub(super) fn accepted_plan(&self, record: &IdempotencyRecord, kind: &str) -> Value {
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

    pub(super) fn envelope(&self, operation: &str, value: Value) -> Value {
        json!({"schema":SESSION_API_SCHEMA,"operation":operation,"value":value,"effect_class":"local_metadata_only","inference_calls":0,"game_effects":0})
    }
}
