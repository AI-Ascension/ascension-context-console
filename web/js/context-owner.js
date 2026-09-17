// SPDX-License-Identifier: MIT

const MAX_RESPONSE_BYTES = 1024 * 1024;
const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
const SHA256 = /^[0-9a-f]{64}$/;
const ASSOCIATION_SCHEMA = "ascension.harness.context-owner-association-view.v1";
const LIMITS_SCHEMA = "ascension.harness.context-owner-effective-limits-view.v1";
const RECEIPT_SCHEMA = "ascension.context-control.owner-receipt.v2";

export class ContextOwnerError extends Error {
  constructor(message, status = 0, code = "context_owner_error") {
    super(message);
    this.name = "ContextOwnerError";
    this.status = status;
    this.code = code;
  }
}

function object(value, fields, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).sort().join("\0") !== [...fields].sort().join("\0")) {
    throw new ContextOwnerError(`${label} has an invalid shape`, 502, "invalid_context_owner_response");
  }
  return value;
}

function identifier(value, label) {
  if (typeof value !== "string" || !IDENTIFIER.test(value)) {
    throw new ContextOwnerError(`${label} is invalid`, 502, "invalid_context_owner_response");
  }
}

function digest(value, label) {
  if (typeof value !== "string" || !SHA256.test(value)) {
    throw new ContextOwnerError(`${label} is invalid`, 502, "invalid_context_owner_response");
  }
}

function positive(value, label) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new ContextOwnerError(`${label} is invalid`, 502, "invalid_context_owner_response");
  }
}

function nonNegative(value, label) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new ContextOwnerError(`${label} is invalid`, 502, "invalid_context_owner_response");
  }
}

function boolean(value, label) {
  if (typeof value !== "boolean") {
    throw new ContextOwnerError(`${label} is invalid`, 502, "invalid_context_owner_response");
  }
}

function validateInputs(runId, token) {
  if (typeof runId !== "string" || !IDENTIFIER.test(runId)) {
    throw new ContextOwnerError("Enter a valid workflow run ID", 400, "invalid_workflow_run_id");
  }
  if (typeof token !== "string" || token.length === 0 || token.length > 4096 || /[\r\n]/.test(token)) {
    throw new ContextOwnerError("Enter the workflow owner bearer token", 400, "owner_token_required");
  }
}

function validateBoundary(boundary) {
  object(boundary, [
    "run_id", "episode_id", "agent_id", "state_id", "generation",
    "observation_sha256", "catalog_sha256", "adapter_revision", "model_revision",
    "configuration_sha256", "output_schema_sha256", "controller_epoch", "gate_epoch",
    "control_version",
  ], "Context boundary");
  for (const [key, label] of [
    ["run_id", "Boundary run ID"], ["episode_id", "Boundary episode ID"],
    ["agent_id", "Boundary agent ID"], ["state_id", "Boundary state ID"],
    ["adapter_revision", "Boundary adapter revision"], ["model_revision", "Boundary model revision"],
  ]) identifier(boundary[key], label);
  for (const [key, label] of [
    ["observation_sha256", "Boundary observation digest"],
    ["catalog_sha256", "Boundary catalog digest"],
    ["configuration_sha256", "Boundary configuration digest"],
    ["output_schema_sha256", "Boundary output-schema digest"],
  ]) digest(boundary[key], label);
  nonNegative(boundary.generation, "Boundary generation");
  positive(boundary.controller_epoch, "Boundary controller epoch");
  nonNegative(boundary.gate_epoch, "Boundary gate epoch");
  nonNegative(boundary.control_version, "Boundary control version");
}

function validateBinding(binding, runId) {
  object(binding, [
    "schema_version", "owner_id", "owner_version", "invocation_id", "binding_id",
    "binding_version", "binding_digest", "context_ref", "instance_id", "node_kind",
    "state", "workflow_run_id", "definition_digest", "graph_id", "node_id",
    "node_execution_id", "boundary", "lease_epoch", "snapshot_id", "approved_revision_id",
    "plan_epoch", "grants", "continuity",
  ], "Context owner binding");
  validateBoundary(binding.boundary);
  if (binding.schema_version !== "ascension.context-control.owner-binding.v1"
    || binding.state !== "available" || binding.workflow_run_id !== runId
    || binding.boundary.run_id !== runId) {
    throw new ContextOwnerError("Context owner returned a stale or unavailable binding", 409, "context_binding_mismatch");
  }
  for (const [key, label] of [
    ["owner_id", "Owner ID"], ["owner_version", "Owner version"], ["invocation_id", "Invocation ID"],
    ["binding_id", "Binding ID"], ["context_ref", "Context reference"], ["instance_id", "Instance ID"],
    ["node_kind", "Node kind"], ["workflow_run_id", "Workflow run ID"], ["graph_id", "Graph ID"],
    ["node_id", "Node ID"], ["node_execution_id", "Node execution ID"],
    ["snapshot_id", "Snapshot ID"], ["approved_revision_id", "Approved revision ID"],
  ]) identifier(binding[key], label);
  positive(binding.binding_version, "Binding version");
  positive(binding.lease_epoch, "Binding lease epoch");
  positive(binding.plan_epoch, "Binding plan epoch");
  digest(binding.binding_digest, "Binding digest");
  digest(binding.definition_digest, "Definition digest");
  object(binding.grants, ["metadata_read", "content_read", "edit", "control"], "Binding grants");
  object(binding.continuity, [
    "survives_controller_restart", "receipt_recovery", "provider_session_continuity",
  ], "Binding continuity");
  for (const value of Object.values(binding.grants)) boolean(value, "Binding grant");
  for (const value of Object.values(binding.continuity)) boolean(value, "Binding continuity flag");
  return binding;
}

function validateAssociation(response, runId) {
  object(response, ["schema_version", "binding"], "Context owner association");
  if (response.schema_version !== ASSOCIATION_SCHEMA) {
    throw new ContextOwnerError("Context owner returned an unsupported association schema", 502, "invalid_context_owner_response");
  }
  validateBinding(response.binding, runId);
  return response;
}

function validateLimits(response, _runId) {
  object(response, [
    "schema_version", "owner_id", "owner_version", "catalog_digest", "binding_id",
    "binding_version", "binding_digest", "context_ref", "node_kind", "adapter_revision",
    "model_revision", "effective_limits",
  ], "Context owner effective limits");
  if (response.schema_version !== LIMITS_SCHEMA) {
    throw new ContextOwnerError("Context owner returned an unsupported limits schema", 502, "invalid_context_owner_response");
  }
  for (const [key, label] of [
    ["owner_id", "Owner ID"], ["owner_version", "Owner version"], ["binding_id", "Binding ID"],
    ["context_ref", "Context reference"], ["node_kind", "Node kind"],
    ["adapter_revision", "Adapter revision"], ["model_revision", "Model revision"],
  ]) identifier(response[key], label);
  digest(response.catalog_digest, "Catalog digest");
  digest(response.binding_digest, "Binding digest");
  positive(response.binding_version, "Binding version");
  object(response.effective_limits, [
    "max_items", "max_notes", "max_context_bytes", "max_objective_bytes", "max_control_events",
  ], "Effective limits");
  for (const [key, label] of [
    ["max_items", "Maximum items"], ["max_notes", "Maximum notes"],
    ["max_context_bytes", "Maximum context bytes"], ["max_objective_bytes", "Maximum objective bytes"],
    ["max_control_events", "Maximum control events"],
  ]) positive(response.effective_limits[key], label);
  return response;
}

function validateCommand(command) {
  if (!command || typeof command !== "object" || Array.isArray(command)
    || Object.keys(command).length !== 1) {
    throw new ContextOwnerError("Enter one tagged context control command", 400, "context_control_command_required");
  }
  const kind = Object.keys(command)[0];
  if (!["pause", "commit", "resume"].includes(kind)) {
    throw new ContextOwnerError("Enter one tagged context control command", 400, "context_control_command_required");
  }
  const fields = {
    pause: ["idempotency_key", "expected_control_version"],
    commit: [
      "idempotency_key", "expected_control_version", "expected_revision_id",
      "expected_boundary", "preview_manifest_digest", "approved_manifest_digest",
    ],
    resume: ["idempotency_key", "expected_control_version", "expected_boundary"],
  }[kind];
  const value = command[kind];
  object(value, fields, `${kind} context control command`);
  identifier(value.idempotency_key, "Command idempotency key");
  nonNegative(value.expected_control_version, "Command expected control version");
  if (kind === "commit") {
    identifier(value.expected_revision_id, "Command expected revision ID");
    validateBoundary(value.expected_boundary);
    digest(value.preview_manifest_digest, "Command preview manifest digest");
    digest(value.approved_manifest_digest, "Command approved manifest digest");
  } else if (kind === "resume") {
    validateBoundary(value.expected_boundary);
  }
  return { kind, value };
}

function validateReceipt(receipt, command, runId) {
  object(receipt, [
    "schema_version", "owner_id", "invocation_id", "binding_id", "binding_digest", "command",
    "command_id", "idempotency_key", "effect", "control_version", "plan_epoch",
    "controller_epoch", "gate_epoch", "boundary", "revision_id", "preview_manifest_digest",
    "approved_manifest_digest",
  ], "Context control receipt");
  const { kind, value: commandValue } = validateCommand(command);
  validateBoundary(receipt.boundary);
  if (receipt.schema_version !== RECEIPT_SCHEMA
    || receipt.command !== kind
    || receipt.idempotency_key !== commandValue.idempotency_key
    || receipt.boundary.run_id !== runId) {
    throw new ContextOwnerError("Recovered receipt does not match the requested command", 409, "context_control_receipt_mismatch");
  }
  for (const [key, label] of [
    ["owner_id", "Receipt owner ID"], ["invocation_id", "Receipt invocation ID"],
    ["binding_id", "Receipt binding ID"], ["command_id", "Receipt command ID"],
    ["idempotency_key", "Receipt idempotency key"], ["effect", "Receipt effect"],
  ]) identifier(receipt[key], label);
  digest(receipt.binding_digest, "Receipt binding digest");
  for (const [key, label] of [
    ["control_version", "Receipt control version"], ["plan_epoch", "Receipt plan epoch"],
    ["controller_epoch", "Receipt controller epoch"], ["gate_epoch", "Receipt gate epoch"],
  ]) nonNegative(receipt[key], label);
  if (kind === "commit") {
    if (receipt.revision_id === null || receipt.preview_manifest_digest === null
      || receipt.approved_manifest_digest === null) {
      throw new ContextOwnerError("Commit receipt is missing revision identity", 409, "context_control_receipt_mismatch");
    }
    identifier(receipt.revision_id, "Receipt revision ID");
    digest(receipt.preview_manifest_digest, "Receipt preview manifest digest");
    digest(receipt.approved_manifest_digest, "Receipt approved manifest digest");
    if (receipt.preview_manifest_digest !== commandValue.preview_manifest_digest
      || receipt.approved_manifest_digest !== commandValue.approved_manifest_digest) {
      throw new ContextOwnerError("Recovered receipt does not match the requested command", 409, "context_control_receipt_mismatch");
    }
  } else if (receipt.revision_id !== null
    || receipt.preview_manifest_digest !== null
    || receipt.approved_manifest_digest !== null) {
    throw new ContextOwnerError("Non-commit receipt carries commit identity", 409, "context_control_receipt_mismatch");
  }
  return receipt;
}

async function boundedResponseText(response) {
  if (!response.body) return "";
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > MAX_RESPONSE_BYTES) {
        await reader.cancel();
        throw new ContextOwnerError("Context owner response exceeds the local byte bound", 502, "context_owner_response_too_large");
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return new TextDecoder().decode(bytes);
}

async function ownerRequest(runId, token, suffix, options = {}) {
  validateInputs(runId, token);
  const response = await fetch(`/v1/workflow-runs/${encodeURIComponent(runId)}${suffix}`, {
    ...options,
    credentials: "same-origin",
    cache: "no-store",
    headers: {
      Accept: "application/json",
      Authorization: `Bearer ${token}`,
      ...(options.headers || {}),
    },
  });
  const text = await boundedResponseText(response);
  let body;
  try { body = text ? JSON.parse(text) : undefined; } catch {
    throw new ContextOwnerError("Context owner returned invalid JSON", response.status || 502, "invalid_context_owner_response");
  }
  if (!response.ok) {
    const message = body?.error?.message || body?.error?.code;
    throw new ContextOwnerError(
      typeof message === "string" ? message : `Context owner returned HTTP ${response.status}`,
      response.status,
      typeof body?.error?.code === "string" ? body.error.code : "context_owner_http_error",
    );
  }
  return body;
}

export async function getContextOwnerAssociation(runId, token) {
  return validateAssociation(await ownerRequest(runId, token, "/context-owner-association"), runId);
}

export async function getContextOwnerEffectiveLimits(runId, token) {
  return validateLimits(await ownerRequest(runId, token, "/context-owner-effective-limits"), runId);
}

export async function recoverContextControlReceipt(runId, token, command) {
  validateCommand(command);
  const response = await ownerRequest(runId, token, "/context-control-receipts/lookup", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(command),
  });
  return validateReceipt(response, command, runId);
}
