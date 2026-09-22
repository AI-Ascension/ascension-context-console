// SPDX-License-Identifier: MIT

use super::*;

pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

pub(super) fn safe_generation(value: u64) -> bool {
    value <= MAX_SAFE_INTEGER
}

pub(super) fn owner_session_payload(
    operation: OwnerOperation,
    method: &str,
    body: &[u8],
) -> Result<Value, SessionApiError> {
    if operation.read_only() {
        if method != "GET" || !body.is_empty() {
            return Err(if method == "GET" {
                SessionApiError::BadRequest
            } else {
                SessionApiError::MethodNotAllowed
            });
        }
        return Ok(json!({}));
    }
    if method != "POST" {
        return Err(SessionApiError::MethodNotAllowed);
    }
    match operation {
        OwnerOperation::SessionCandidate => {
            parse_typed_session_command(body, |command: &SessionCandidateCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.approved_policy_ref)
                    && valid_id(&command.profile_ref)
            })
        }
        OwnerOperation::SessionHistoryRefresh => {
            parse_typed_session_command(body, |command: &SessionHistoryRefreshCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
            })
        }
        OwnerOperation::SessionReconnect => {
            parse_typed_session_command(body, |command: &SessionReconnectCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
            })
        }
        OwnerOperation::SessionForkPlan => {
            parse_typed_session_command(body, |command: &SessionForkPlanCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
                    && valid_id(&command.cutoff_turn_ref)
            })
        }
        OwnerOperation::SessionCompactionPlan => {
            parse_typed_session_command(body, |command: &SessionCompactionPlanCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
            })
        }
        OwnerOperation::SessionPreparedBinding => {
            parse_typed_session_command(body, |command: &SessionPreparedBindingCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
                    && safe_generation(command.expected_history_epoch)
                    && valid_id(&command.phase2_draft_ref)
                    && valid_id(&command.phase3_selection_ref)
            })
        }
        OwnerOperation::SessionFork => {
            parse_typed_session_command(body, |command: &SessionForkCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.approved_fork_plan_ref)
            })
        }
        OwnerOperation::SessionCompaction => {
            parse_typed_session_command(body, |command: &SessionCompactionCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.approved_compaction_plan_ref)
                    && valid_id(&command.spend_authorization_ref)
            })
        }
        OwnerOperation::SessionRetire => {
            parse_typed_session_command(body, |command: &SessionRetireCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && safe_generation(command.expected_session_epoch)
            })
        }
        OwnerOperation::SessionCleanup => {
            parse_typed_session_command(body, |command: &SessionCleanupCommand| {
                valid_id(&command.idempotency_key)
                    && safe_generation(command.expected_control_generation)
                    && valid_id(&command.retirement_ref)
                    && valid_id(&command.erase_authorization_ref)
                    && safe_generation(command.expected_session_epoch)
            })
        }
        _ => Err(SessionApiError::Unsupported),
    }
}

pub(super) fn parse_typed_session_command<T, F>(
    body: &[u8],
    validate: F,
) -> Result<Value, SessionApiError>
where
    T: serde::de::DeserializeOwned + Serialize,
    F: FnOnce(&T) -> bool,
{
    if body.is_empty() {
        return Err(SessionApiError::BadRequest);
    }
    let command: T = crate::parse_control_json(body).map_err(|_| SessionApiError::BadRequest)?;
    if !validate(&command) {
        return Err(SessionApiError::BadRequest);
    }
    let value = serde_json::to_value(command).map_err(|_| SessionApiError::BadRequest)?;
    validate_public_value(&value).map_err(|_| SessionApiError::BadRequest)?;
    Ok(value)
}

pub(super) fn owner_session_result(
    operation: OwnerOperation,
    revocation_epoch: u64,
    reply: crate::owner::OwnerReply,
) -> Result<Value, SessionApiError> {
    let receipt =
        serde_json::to_value(&reply.receipt).map_err(|_| SessionApiError::MalformedPeer)?;
    let outcome = reply.receipt.outcome;
    let value = match reply.value {
        Some(value) => value,
        None => Value::Null,
    };
    Ok(json!({
        "schema": SESSION_API_SCHEMA,
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
        "owner_receipt": receipt,
        "value": value,
    }))
}

pub(super) fn scope_for_run(run_id: &str) -> SessionScopeView {
    SessionScopeView {
        project_id: "project-fixture".to_owned(),
        run_id: run_id.to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    }
}

pub(super) fn fixture_expiry() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    crate::control::format_time(now.saturating_add(FIXTURE_EXPIRY_SECONDS))
}

pub(super) fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    use sha2::{Digest as _, Sha256};
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(super) fn parse_command(
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

pub(super) fn require_id(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), SessionApiError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| valid_id(value))
        .map(|_| ())
        .ok_or(SessionApiError::BadRequest)
}

pub(super) fn require_u64(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), SessionApiError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .map(|_| ())
        .ok_or(SessionApiError::BadRequest)
}

#[allow(clippy::manual_unwrap_or_default)]
pub(super) fn canonical_digest(value: &Value) -> String {
    let canonical = canonical_value(value);
    let bytes = match serde_json::to_vec(&canonical) {
        Ok(bytes) => bytes,
        Err(_) => Vec::new(),
    };
    sha256_hex(bytes)
}

pub(super) fn canonical_value(value: &Value) -> Value {
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
