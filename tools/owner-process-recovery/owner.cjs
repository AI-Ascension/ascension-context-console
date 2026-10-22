// SPDX-License-Identifier: MIT
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { request, successful } = require("./http.cjs");

function canonical(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  return `{${Object.entries(value).sort(([a], [b]) => a.localeCompare(b)).map(([key, entry]) => `${JSON.stringify(key)}:${canonical(entry)}`).join(",")}}`;
}

// Fixture setup uses Harness's real admission/source endpoints. All assertions
// under test after setup traverse the independent Console operator process.
async function prepareOwner(stack) {
  const proxyAddress = new URL(stack.baseUrl).host;
  const fixture = successful(await request(proxyAddress, "/__fixture", { headers: stack.fixtureControlHeaders }), "fixture metadata");
  const call = async (url, method = "GET", body) => successful(await request(stack.ownerAddress, url, { method, body, headers: stack.ownerHeaders }), url);
  const runPath = `/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}`;
  const definition = JSON.parse(fs.readFileSync(path.join(process.env.CONSOLE_POLICY_HARNESS_ROOT, "conformance/workflow-v1/valid-strict.json"), "utf8"));
  definition.annotations.synthetic = false;
  definition.game_profile = "sts2-live-v1";
  definition.policy_ref = "policy.live.v1";
  definition.graphs[0].nodes[0].config.projection_ref = "fair-play.live.v1";
  definition.graphs[0].nodes[1].config.decision_profile_ref = "decision.live.v1";
  definition.graphs[0].nodes[1].config.context_ref = "context.live.v1";
  definition.capabilities.required[0] = "observe.fair-play.v1";
  const definitionDigest = crypto.createHash("sha256").update(canonical(definition)).digest("hex");
  assert.equal(definitionDigest, fixture.definition_digest);
  const catalog = await call("/v1/workflow-targets");
  const descriptor = catalog.targets.find((target) => target.instance_id === fixture.instance_id);
  assert.ok(descriptor);
  const target = {
    instance_id: descriptor.instance_id, execution_profile: "live.workflow.v1", execution_mode: "live", workflow_revision: definition.version,
    compatibility_revision: descriptor.compatibility_revision, capability_revision: descriptor.capability_revision,
    game_profile: "sts2-live-v1", save_profile: null, inference_profile: null, context_capability: null, provider_capability: null,
  };
  const preflight = await call("/v1/workflow-targets/preflight", "POST", {
    schema_version: "ascension.workflow-admission/v1", request_id: fixture.request_id, workflow_definition_digest: definitionDigest, target,
  });
  const submission = await call("/v1/workflow-runs", "POST", {
    schema_version: "ascension.management/v1", request_id: fixture.request_id, definition, artifact_id: null, instance_id: fixture.instance_id, profile: "live.workflow.v1", admission: preflight.admission,
  });
  assert.equal(submission.workflow_run_id, fixture.run_id);
  const admitted = await call(runPath);
  await call(`${runPath}/commands`, "POST", {
    schema_version: "ascension.management/v1", command_id: "console-process-observe", run_id: fixture.run_id,
    expected_revision: admitted.run.run_revision, actor_scope: "profile:console-live", kind: "step", parameters: {},
  });
  const current = await call(runPath);
  assert.equal(current.run.cursor.node_id, "decide");
  const bindings = await call("/v1/context-bindings");
  const binding = bindings.descriptors.find((item) => item.context_ref === "context.live.v1" && item.node_kinds.includes("decide"));
  assert.ok(binding);
  await call("/v1/context-bindings/bind", "POST", {
    workflow_run_id: fixture.run_id, definition_digest: current.run.definition_digest, instance_id: fixture.instance_id,
    graph_id: current.run.cursor.graph_id, node_id: current.run.cursor.node_id, node_execution_id: current.run.cursor.node_execution_id,
    node_kind: "decide", context_ref: "context.live.v1", binding_id: binding.binding_id, binding_version: binding.version, binding_digest: binding.digest,
  });
  await call(`${runPath}/context-sources/strategy`, "PUT", { schema_version: "ascension.context-owner.context-source-upload.v1", document: stack.contextSourceDocument });
  const source = await call(`${runPath}/context-owner-source-status`);
  await call(`${runPath}/context-sources/strategy/adopt`, "POST", {
    schema_version: "ascension.context-owner.context-source-adoption.v1", idempotency_key: "console-process-source-adoption",
    expected_control_version: source.boundary.control_version, expected_revision_id: source.active_revision_id, expected_boundary: source.boundary,
  });
  return {
    runId: fixture.run_id, runPath,
    effects: async () => {
      const value = successful(await request(proxyAddress, "/__fixture", { headers: stack.fixtureControlHeaders }), "effect counters");
      return { mod: value.synthetic_mod_effects, provider: value.provider_bridge_invocations };
    },
  };
}
module.exports = { prepareOwner };
