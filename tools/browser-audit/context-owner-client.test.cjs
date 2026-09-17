// SPDX-License-Identifier: MIT

const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");

let client;

test.before(async () => {
  const sourcePath = path.resolve(__dirname, "../../web/js/context-owner.js");
  const source = await fs.readFile(sourcePath, "utf8");
  client = await import(`data:text/javascript,${encodeURIComponent(source)}`);
});

const digest = "a".repeat(64);

function boundary(runId = "run-live-1") {
  return {
    run_id: runId,
    episode_id: "episode-1",
    agent_id: "agent-1",
    state_id: "state-1",
    generation: 0,
    observation_sha256: digest,
    catalog_sha256: digest,
    adapter_revision: "adapter-1",
    model_revision: "model-1",
    configuration_sha256: digest,
    output_schema_sha256: digest,
    controller_epoch: 0,
    gate_epoch: 0,
    control_version: 0,
  };
}

function pauseReceipt(runId = "run-live-1", command = "pause") {
  return {
    schema_version: "ascension.context-control.owner-receipt.v2",
    owner_id: "owner-1",
    invocation_id: "invocation-1",
    binding_id: "binding-1",
    binding_digest: digest,
    command,
    command_id: "command-1",
    idempotency_key: "idempotency-1",
    effect: "pause_requested",
    control_version: 0,
    plan_epoch: 0,
    controller_epoch: 0,
    gate_epoch: 0,
    boundary: boundary(runId),
    revision_id: null,
    preview_manifest_digest: null,
    approved_manifest_digest: null,
  };
}

test("receipt lookup rejects untagged or extra-field commands before network access", async () => {
  let fetchCalls = 0;
  const originalFetch = global.fetch;
  global.fetch = async () => {
    fetchCalls += 1;
    return new Response("{}");
  };
  try {
    await assert.rejects(
      client.recoverContextControlReceipt("run-live-1", "token", {
        idempotency_key: "idempotency-1",
      }),
      (error) => error.code === "context_control_command_required",
    );
    await assert.rejects(
      client.recoverContextControlReceipt("run-live-1", "token", {
        pause: { idempotency_key: "idempotency-1", expected_control_version: 0, extra: true },
      }),
      (error) => error.code === "invalid_context_owner_response",
    );
    assert.equal(fetchCalls, 0);
  } finally {
    global.fetch = originalFetch;
  }
});

test("receipt lookup enforces tagged command and selected run boundary identity", async () => {
  const originalFetch = global.fetch;
  const command = { pause: { idempotency_key: "idempotency-1", expected_control_version: 0 } };
  let requestBody;
  let response = pauseReceipt("run-live-1", "resume");
  global.fetch = async (_url, options) => {
    requestBody = JSON.parse(options.body);
    const current = response;
    response = pauseReceipt("run-other");
    return new Response(JSON.stringify(current));
  };
  try {
    await assert.rejects(
      client.recoverContextControlReceipt("run-live-1", "token", command),
      (error) => error.code === "context_control_receipt_mismatch",
    );
    assert.deepEqual(requestBody, command);
    await assert.rejects(
      client.recoverContextControlReceipt("run-live-1", "token", command),
      (error) => error.code === "context_control_receipt_mismatch",
    );
  } finally {
    global.fetch = originalFetch;
  }
});

test("bounded owner responses reject bodies above the client limit", async () => {
  const originalFetch = global.fetch;
  global.fetch = async () => new Response("x".repeat(1024 * 1024 + 1));
  try {
    await assert.rejects(
      client.getContextOwnerAssociation("run-live-1", "token"),
      (error) => error.code === "context_owner_response_too_large",
    );
  } finally {
    global.fetch = originalFetch;
  }
});
