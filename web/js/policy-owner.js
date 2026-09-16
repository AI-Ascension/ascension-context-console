// SPDX-License-Identifier: MIT

const MAX_POLICY_BYTES = 1024 * 1024;
const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const POLICY_VIEW_SCHEMA = "ascension.provider-session.policy-owner-view.v1";
const POLICY_COMMAND_SCHEMA = "ascension.provider-session.policy-owner-command.v1";

export const maxPolicyBytes = MAX_POLICY_BYTES;

export class PolicyOwnerError extends Error {
  constructor(message, status = 0, code = "provider_session_policy_error") {
    super(message);
    this.name = "PolicyOwnerError";
    this.status = status;
    this.code = code;
  }
}

function object(value, fields, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).sort().join("\0") !== [...fields].sort().join("\0")) {
    throw new PolicyOwnerError(`${label} has an invalid shape`, 502, "invalid_provider_session_policy_response");
  }
  return value;
}

function identifier(value, label) {
  if (typeof value !== "string" || !IDENTIFIER.test(value)) {
    throw new PolicyOwnerError(`${label} is invalid`, 502, "invalid_provider_session_policy_response");
  }
}

function digest(value, label) {
  if (typeof value !== "string" || !SHA256.test(value)) {
    throw new PolicyOwnerError(`${label} is invalid`, 502, "invalid_provider_session_policy_response");
  }
}

function safeCount(value, label, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum) {
    throw new PolicyOwnerError(`${label} is invalid`, 502, "invalid_provider_session_policy_response");
  }
}

function enumValue(value, choices, label) {
  if (!choices.includes(value)) {
    throw new PolicyOwnerError(`${label} is invalid`, 502, "invalid_provider_session_policy_response");
  }
}

function validatePolicy(policy, active = false) {
  const fields = active
    ? ["sha256", "policy_id", "version", "mode", "continuity", "max_completed_turns", "history_ttl_seconds", "epoch"]
    : ["sha256", "policy_id", "version", "mode", "continuity", "active"];
  object(policy, fields, "Policy metadata");
  digest(policy.sha256, "Policy digest");
  identifier(policy.policy_id, "Policy ID");
  safeCount(policy.version, "Policy version", 1);
  enumValue(policy.mode, ["disabled", "fixture_only", "inspect_only", "enabled"], "Policy mode");
  enumValue(policy.continuity, ["strict_reviewed", "observed_persistent"], "Policy continuity");
  if (active) {
    safeCount(policy.max_completed_turns, "Maximum completed turns");
    safeCount(policy.history_ttl_seconds, "History TTL");
    safeCount(policy.epoch, "Policy epoch", 1);
  } else if (typeof policy.active !== "boolean") {
    throw new PolicyOwnerError("Policy history active flag is invalid", 502, "invalid_provider_session_policy_response");
  }
}

function validateProposal(proposal) {
  object(proposal, [
    "proposal_id",
    "proposal_sha256",
    "source_sha256",
    "target_sha256",
    "state",
    "approval_recorded",
    "adopted_policy_sha256",
  ], "Proposal metadata");
  identifier(proposal.proposal_id, "Proposal ID");
  digest(proposal.proposal_sha256, "Proposal digest");
  digest(proposal.source_sha256, "Proposal source digest");
  digest(proposal.target_sha256, "Proposal target digest");
  enumValue(proposal.state, ["proposed", "approved", "adopted"], "Proposal state");
  if (typeof proposal.approval_recorded !== "boolean") {
    throw new PolicyOwnerError("Proposal approval flag is invalid", 502, "invalid_provider_session_policy_response");
  }
  if (proposal.adopted_policy_sha256 !== null) digest(proposal.adopted_policy_sha256, "Adopted policy digest");
}

function validateView(response, runId) {
  object(response, ["schema_version", "operation", "value", "effect_class", "inference_calls", "game_effects"], "Policy view response");
  if (response.schema_version !== POLICY_VIEW_SCHEMA || response.operation !== "current"
    || response.effect_class !== "local_metadata_only" || response.inference_calls !== 0 || response.game_effects !== 0) {
    throw new PolicyOwnerError("Policy owner returned an unsupported response", 502, "invalid_provider_session_policy_response");
  }
  const value = object(response.value, ["run_id", "revision", "active", "history", "proposals"], "Policy owner view");
  if (value.run_id !== runId) throw new PolicyOwnerError("Policy owner returned a different workflow run", 409, "provider_session_policy_run_mismatch");
  identifier(value.run_id, "Workflow run ID");
  safeCount(value.revision, "Owner revision", 1);
  if (value.active !== null) validatePolicy(value.active, true);
  if (!Array.isArray(value.history) || value.history.length > 64
    || !Array.isArray(value.proposals) || value.proposals.length > 64) {
    throw new PolicyOwnerError("Policy owner history exceeds its response bound", 502, "invalid_provider_session_policy_response");
  }
  value.history.forEach((policy) => validatePolicy(policy));
  value.proposals.forEach(validateProposal);
  return response;
}

function validateCommand(response, operation) {
  object(response, [
    "schema_version",
    "operation",
    "revision",
    "policy_sha256",
    "proposal_sha256",
    "effect_class",
    "inference_calls",
    "game_effects",
  ], "Policy command response");
  if (response.schema_version !== POLICY_COMMAND_SCHEMA || response.operation !== operation
    || response.effect_class !== "local_metadata_only" || response.inference_calls !== 0 || response.game_effects !== 0) {
    throw new PolicyOwnerError("Policy owner returned an unsupported command response", 502, "invalid_provider_session_policy_response");
  }
  safeCount(response.revision, "Owner revision", 1);
  if (response.policy_sha256 !== null) digest(response.policy_sha256, "Policy digest");
  if (response.proposal_sha256 !== null) digest(response.proposal_sha256, "Proposal digest");
  const expectsPolicyDigest = operation === "import" || operation === "adopt";
  const expectsProposalDigest = operation === "propose";
  if ((expectsPolicyDigest !== (response.policy_sha256 !== null))
    || (expectsProposalDigest !== (response.proposal_sha256 !== null))) {
    throw new PolicyOwnerError("Policy command response digest does not match its operation", 502, "invalid_provider_session_policy_response");
  }
  return response;
}

function validateInputs(runId, token) {
  if (typeof runId !== "string" || !IDENTIFIER.test(runId)) {
    throw new PolicyOwnerError("Enter a valid workflow run ID", 400, "invalid_workflow_run_id");
  }
  if (typeof token !== "string" || token.length === 0 || token.length > 4096 || /[\r\n]/.test(token)) {
    throw new PolicyOwnerError("Enter the workflow owner bearer token", 400, "owner_token_required");
  }
}

async function ownerRequest(runId, token, suffix, options = {}) {
  validateInputs(runId, token);
  const response = await fetch(`/v1/workflow-runs/${encodeURIComponent(runId)}/provider-session-policy${suffix}`, {
    ...options,
    credentials: "same-origin",
    cache: "no-store",
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
      ...(options.headers || {}),
    },
  });
  const text = await response.text();
  if (new TextEncoder().encode(text).length > MAX_POLICY_BYTES) {
    throw new PolicyOwnerError("Policy owner response exceeds the local byte bound", 502, "provider_session_policy_response_too_large");
  }
  let body;
  try {
    body = text ? JSON.parse(text) : undefined;
  } catch {
    throw new PolicyOwnerError("Policy owner returned invalid JSON", response.status || 502, "invalid_provider_session_policy_response");
  }
  if (!response.ok) {
    const message = body?.error?.message || body?.error?.code;
    throw new PolicyOwnerError(
      typeof message === "string" ? message : `Policy owner returned HTTP ${response.status}`,
      response.status,
      typeof body?.error?.code === "string" ? body.error.code : "provider_session_policy_http_error",
    );
  }
  return body;
}

function revisionQuery(revision) {
  safeCount(revision, "Expected owner revision", 1);
  return `?expected_revision=${encodeURIComponent(String(revision))}`;
}

function policyBody(field, value) {
  if (field === "policy_sha256" || field === "proposal_sha256") digest(value, field);
  if (field === "approval_ref") identifier(value, "Approval reference");
  return JSON.stringify({
    schema_version: POLICY_COMMAND_SCHEMA,
    [field]: value,
  });
}

function boundedPolicyBytes(bytes) {
  if (!(bytes instanceof ArrayBuffer) || bytes.byteLength === 0 || bytes.byteLength > MAX_POLICY_BYTES) {
    throw new PolicyOwnerError(`Choose a non-empty JSON policy no larger than ${MAX_POLICY_BYTES} bytes`, 400, "provider_session_policy_upload_bound");
  }
}

export async function getProviderSessionPolicy(runId, token) {
  return validateView(await ownerRequest(runId, token, ""), runId);
}

export async function importProviderSessionPolicy(runId, token, revision, bytes) {
  boundedPolicyBytes(bytes);
  const response = await ownerRequest(runId, token, `/import${revisionQuery(revision)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: bytes,
  });
  return validateCommand(response, "import");
}

export async function proposeProviderSessionPolicy(runId, token, proposalId, sourceSha256, revision, bytes) {
  identifier(proposalId, "Proposal ID");
  digest(sourceSha256, "Source policy digest");
  safeCount(revision, "Expected owner revision", 1);
  boundedPolicyBytes(bytes);
  const query = new URLSearchParams({
    source_sha256: sourceSha256,
    expected_revision: String(revision),
  });
  const response = await ownerRequest(
    runId,
    token,
    `/proposals/${encodeURIComponent(proposalId)}?${query.toString()}`,
    { method: "POST", headers: { "Content-Type": "application/json" }, body: bytes },
  );
  return validateCommand(response, "propose");
}

export async function approveProviderSessionPolicy(runId, token, proposalId, proposalSha256, approvalRef, revision) {
  identifier(proposalId, "Proposal ID");
  identifier(approvalRef, "Approval reference");
  const body = policyBody("proposal_sha256", proposalSha256);
  const request = JSON.parse(body);
  request.approval_ref = approvalRef;
  const response = await ownerRequest(runId, token, `/proposals/${encodeURIComponent(proposalId)}/approve${revisionQuery(revision)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(request),
  });
  return validateCommand(response, "approve");
}

export async function adoptProviderSessionPolicyProposal(runId, token, proposalId, proposalSha256, approvalRef, revision) {
  identifier(proposalId, "Proposal ID");
  identifier(approvalRef, "Approval reference");
  const body = policyBody("proposal_sha256", proposalSha256);
  const request = JSON.parse(body);
  request.approval_ref = approvalRef;
  const response = await ownerRequest(runId, token, `/proposals/${encodeURIComponent(proposalId)}/adopt${revisionQuery(revision)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(request),
  });
  return validateCommand(response, "adopt");
}

export async function adoptImportedProviderSessionPolicy(runId, token, policySha256, revision) {
  const response = await ownerRequest(runId, token, `/adoptions${revisionQuery(revision)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: policyBody("policy_sha256", policySha256),
  });
  return validateCommand(response, "adopt");
}
