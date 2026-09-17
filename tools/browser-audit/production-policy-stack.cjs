// SPDX-License-Identifier: MIT

const crypto = require("node:crypto");
const fs = require("node:fs");
const http = require("node:http");
const net = require("node:net");
const path = require("node:path");
const { execFileSync, spawn, spawnSync } = require("node:child_process");

function digest(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

const appRoot = path.resolve(__dirname, "../..");
const lock = JSON.parse(fs.readFileSync(path.join(appRoot, "contracts/live-owner-browser.lock.json"), "utf8"));
const token = "console-live-ci-token";
const providerKey = crypto.randomBytes(32).toString("hex");
const contextKey = crypto.randomBytes(32).toString("hex");
const children = new Map();
let closing = false;
let shutdownPromise;
let modServer;
let proxyServer;
let fixtureRoot;
let fixtureInfo;
let helper;
let bridgeCounterPath;
let fixtureControlToken;
let harness;
let gateway;
let demo;
let serviceEnvironment;
let harnessRoot;
let gatewayRoot;
let mcpRoot;
let ownerPort;
let gatewayPort;
let modPort;
let demoPort;
let proxyPort;
let modRequests;
let allocatedPorts;
let gatewayLeaseActive;

function checkedOutRevision(root, expected, label) {
  const actual = execFileSync("git", ["-C", root, "rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const dirty = execFileSync("git", ["-C", root, "status", "--porcelain"], { encoding: "utf8" }).trim();
  if (actual !== expected || dirty) throw new Error(`${label} source checkout does not match its immutable test pin`);
}

function binaryPath(environmentName, root, relativePath) {
  const value = process.env[environmentName]
    ? path.resolve(process.env[environmentName])
    : path.join(root, relativePath);
  if (!fs.statSync(value, { throwIfNoEntry: false })?.isFile()) {
    throw new Error(`${environmentName} must point to the pinned production test binary`);
  }
  return value;
}

function limitedEnvironment(values = {}) {
  return {
    PATH: process.env.PATH || "/usr/bin:/bin",
    ...(process.env.LD_LIBRARY_PATH ? { LD_LIBRARY_PATH: process.env.LD_LIBRARY_PATH } : {}),
    TMPDIR: fixtureRoot,
    ...values,
  };
}

function privateDirectory(parent, prefix) {
  fs.mkdirSync(parent, { recursive: true, mode: 0o700 });
  const directory = fs.mkdtempSync(path.join(parent, prefix));
  fs.chmodSync(directory, 0o700);
  return directory;
}

function startChild(command, args, env, logName, cwd) {
  if (closing) throw new Error("production fixture is shutting down");
  const logPath = path.join(fixtureRoot, logName);
  const output = fs.createWriteStream(logPath, { flags: "a", mode: 0o600 });
  const child = spawn(command, args, {
    cwd,
    env,
    detached: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  const entry = { child, output, stdoutText: "", spawnError: undefined };
  if (Number.isInteger(child.pid)) children.set(child.pid, entry);
  for (const stream of [child.stdout, child.stderr]) {
    stream.on("data", (chunk) => output.write(chunk));
  }
  child.stdout.on("data", (chunk) => {
    entry.stdoutText = `${entry.stdoutText}${chunk.toString("utf8")}`.slice(-32_768);
  });
  child.on("error", (error) => { entry.spawnError = error; });
  child.on("close", () => output.end());
  return entry;
}

function childExited(entry) {
  return !entry?.child || entry.child.exitCode !== null || entry.child.signalCode !== null;
}

function waitForChild(entry, timeoutMillis) {
  if (childExited(entry)) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      entry.child.off("close", onClose);
      resolve();
    }, timeoutMillis);
    function onClose() {
      clearTimeout(timer);
      resolve();
    }
    entry.child.once("close", onClose);
  });
}

function processGroupExists(processGroupId) {
  try {
    process.kill(-processGroupId, 0);
    return true;
  } catch (error) {
    return error?.code === "EPERM";
  }
}

async function waitForProcessGroupExit(processGroupId, timeoutMillis) {
  const deadline = Date.now() + timeoutMillis;
  while (processGroupExists(processGroupId) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  if (processGroupExists(processGroupId)) {
    throw new Error("owned production fixture process group did not exit after bounded cleanup");
  }
}

async function stopChild(entry) {
  if (!entry?.child || !Number.isInteger(entry.child.pid)) return;
  const pid = entry.child.pid;
  if (children.has(pid)) {
    try { process.kill(-pid, "SIGTERM"); } catch {}
    await waitForChild(entry, 3_000);
    if (processGroupExists(pid)) {
      try { process.kill(-pid, "SIGKILL"); } catch {}
    }
    await waitForProcessGroupExit(pid, 2_000);
    children.delete(pid);
  }
  entry.output.end();
}

async function freePort(usedPorts) {
  const server = net.createServer();
  for (let attempt = 0; attempt < 8; attempt += 1) {
    await new Promise((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", resolve);
    });
    const address = server.address();
    if (!address || typeof address === "string") throw new Error("could not reserve a loopback fixture port");
    const port = address.port;
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
    if (!usedPorts.has(port)) {
      usedPorts.add(port);
      return port;
    }
  }
  throw new Error("could not allocate unique loopback fixture ports");
}

function providerConfigFromFixture(info) {
  return JSON.stringify(info.provider_policy_config);
}

function contextSourceFixture() {
  const bytes = Buffer.from("served retained strategy");
  const itemDigest = digest(bytes);
  const document = {
    draft: {
      schema: "ascension.context-control.draft.v1",
      draft_id: "served-draft",
      version: 1,
      base_revision_id: "context.revision.1",
      selected_items: [{ item_id: "served-strategy", version: 1, sha256: itemDigest }],
      pinned_item_ids: [],
      notes: [],
      objective: null,
      author_ref: "operator",
    },
    items: {
      "served-strategy:1": {
        reference: { item_id: "served-strategy", version: 1, sha256: itemDigest },
        kind: "strategy",
        bytes: [...bytes],
        protected: false,
        expires_at: 4_000_000_000,
      },
    },
  };
  const sourceBytes = Buffer.from(JSON.stringify(document));
  return { document, digest: digest(sourceBytes), sourceBytes };
}

function bootstrapFixture() {
  const definitionPath = path.join(harnessRoot, "conformance/workflow-v1/valid-strict.json");
  const sourcePath = path.join(fixtureRoot, "migration-source.json");
  const targetPath = path.join(fixtureRoot, "migration-target.json");
  const storePath = path.join(fixtureRoot, "provider-policy.sqlite3");
  const result = spawnSync(helper, ["bootstrap", storePath, definitionPath, sourcePath, targetPath], {
    cwd: harnessRoot,
    encoding: "utf8",
    env: limitedEnvironment({ STS2_SERVED_PROVIDER_POLICY_KEY: providerKey }),
    maxBuffer: 2 * 1024 * 1024,
  });
  if (result.status !== 0) throw new Error("could not create the private production policy fixture");
  const line = result.stdout.trim().split("\n").at(-1);
  if (!line) throw new Error("production policy fixture helper returned no metadata");
  return { ...JSON.parse(line), source_path: sourcePath, target_path: targetPath, policy_store_path: storePath };
}

function serviceEnv() {
  const contextStorePath = path.join(fixtureRoot, "context-owner.sqlite3");
  return limitedEnvironment({
    STS2_WORKFLOW_LISTEN: `127.0.0.1:${ownerPort}`,
    STS2_WORKFLOW_STORE: path.join(fixtureRoot, "workflow.sqlite3"),
    STS2_WORKFLOW_AUTH_PROFILE: "console-live",
    STS2_WORKFLOW_TOKEN_CONSOLE_LIVE: token,
    STS2_WORKFLOW_PROVIDER_POLICY_CONFIG: providerConfigFromFixture(fixtureInfo),
    STS2_SERVED_PROVIDER_POLICY_KEY: providerKey,
    STS2_WORKFLOW_CONTEXT_OWNER_CONFIG: JSON.stringify({
      schema_version: "ascension.workflow-context-owner-config.v1",
      store_path: contextStorePath,
      key_reference: "STS2_SERVED_CONTEXT_OWNER_KEY",
      owner_id: "console-served-context-owner",
      owner_version: "v1",
      context_ref: "context.live.v1",
      limits: {
        max_items: 64,
        max_notes: 16,
        max_context_bytes: 131072,
        max_objective_bytes: 512,
        max_control_events: 64,
      },
      render_required: true,
      sources: [{ source_id: "strategy", version: 1, digest: fixtureInfo.context_source_digest }],
    }),
    STS2_SERVED_CONTEXT_OWNER_KEY: contextKey,
    STS2_EXECUTION_STORE_PATH: path.join(fixtureRoot, "execution.sqlite3"),
    STS2_GATEWAY_ADDR: `127.0.0.1:${gatewayPort}`,
    STS2_GATEWAY_TOKEN: "console-gateway-token",
    STS2_MCP_BINARY: binaryPath("CONSOLE_POLICY_MCP_BINARY", mcpRoot, "target/debug/sts2-mcp-server"),
    STS2_RUNTIME_PROFILE: "runtime-v4-expert",
    STS2_INSTANCE_ID: fixtureInfo.instance_id,
    STS2_CALLER_ID: "harness",
    STS2_SESSION_ID: "console-production-gateway-session",
    STS2_MCP_SESSION_ID: "console-production-mcp-session",
    STS2_LEASE_ID: "console-production-lease",
    STS2_LEASE_EPOCH: "1",
    STS2_RUN_ID: fixtureInfo.run_id,
    STS2_EPISODE_ID: "episode-served-policy-gate",
    STS2_TRAJECTORY_ID: "trajectory-served-policy-gate",
    STS2_TRACE_ID: "trace-served-policy-gate",
    STS2_ARTIFACT_ID: "artifact-served-policy-gate",
    STS2_EXO_REVISION: "b06869ab789dee3f80ca474b5fa89dbe47ccb859",
    STS2_EXO_ADMISSION: "legacy",
    STS2_EXO_BRIDGE_BINARY: path.join(fixtureRoot, "bounded-exo-bridge.sh"),
    STS2_EXO_TIMEOUT_MILLIS: "2000",
    STS2_EXO_MAX_REQUEST_BYTES: "131072",
    STS2_EXO_MAX_RESPONSE_BYTES: "8192",
    STS2_OBJECTIVE: "exercise production saved-policy routes in Console",
  });
}

function startModServer() {
  const artifactRoot = path.join(harnessRoot, "protocol-artifact");
  const state = JSON.parse(fs.readFileSync(path.join(artifactRoot, "runtime-v3-gameplay/golden/state-response.json"), "utf8"));
  const expert = JSON.parse(fs.readFileSync(path.join(artifactRoot, "runtime-v4-expert/golden/observation.json"), "utf8"));
  const settled = JSON.parse(fs.readFileSync(path.join(artifactRoot, "runtime-v4-expert-action/golden/action-settled.json"), "utf8"));
  modServer = http.createServer(async (request, response) => {
    const chunks = [];
    let size = 0;
    for await (const chunk of request) {
      size += chunk.length;
      if (size > 131_072) {
        response.writeHead(413).end();
        return;
      }
      chunks.push(chunk);
    }
    if (request.headers.authorization !== "Bearer console-mod-token") {
      response.writeHead(401).end();
      return;
    }
    modRequests.push({ method: request.method || "", path: request.url || "" });
    let body = {};
    if (chunks.length) {
      try { body = JSON.parse(Buffer.concat(chunks).toString("utf8")); }
      catch {
        response.writeHead(400).end();
        return;
      }
    }
    let status = 200;
    let value;
    if (request.url === "/api/v3/runtime/state") {
      value = v3Response(state, "state_response", request.headers);
    } else if (request.url === "/api/v3/runtime/legal-actions") {
      value = v3Response(state, "legal_actions_response", request.headers);
    } else if (request.url === "/api/v4/runtime/expert-state") {
      value = { ...expert, state_id: "live:7", generation: 7 };
    } else if (request.url === "/api/v4/runtime/expert-action") {
      status = 503;
      value = { ...settled, status: "unknown", error_code: "transport_timeout", operation_id: body.operation_id };
    } else if (request.url?.startsWith("/api/v4/runtime/expert-actions/")) {
      value = { ...settled, status: "settled" };
    } else {
      status = 404;
      value = { error_code: "fixture_route_missing" };
    }
    const encoded = JSON.stringify(value);
    response.writeHead(status, {
      "content-type": "application/json",
      "content-length": Buffer.byteLength(encoded),
      connection: "close",
    });
    response.end(encoded);
  });
  return new Promise((resolve, reject) => {
    modServer.once("error", reject);
    modServer.listen(modPort, "127.0.0.1", resolve);
  });
}

function gatewayHttpRequest(method, requestPath, body, extraHeaders = {}) {
  return new Promise((resolve, reject) => {
    const request = http.request({
      host: "127.0.0.1",
      port: gatewayPort,
      method,
      path: requestPath,
      headers: {
        authorization: "Bearer console-gateway-token",
        ...(body.length > 0 ? { "content-type": "application/json" } : {}),
        "content-length": body.length,
        ...extraHeaders,
      },
      timeout: 3_000,
    }, (result) => {
      const chunks = [];
      let length = 0;
      result.on("data", (chunk) => {
        length += chunk.length;
        if (length > 32_768) {
          request.destroy(new Error("bounded Gateway readiness response exceeded its limit"));
          return;
        }
        chunks.push(chunk);
      });
      result.on("end", () => resolve({
        status: result.statusCode || 0,
        body: Buffer.concat(chunks),
      }));
    });
    request.on("timeout", () => request.destroy(new Error("Gateway identity probe timed out")));
    request.on("error", reject);
    request.end(body);
  });
}

function gatewayLeaseHeaders(correlationId = "corr-console-production-probe") {
  return {
    "x-sts2-instance-id": fixtureInfo.instance_id,
    "x-sts2-caller-id": "harness",
    "x-sts2-session-id": "console-production-gateway-session",
    "x-mcp-session-id": "console-production-mcp-session",
    "x-sts2-lease-id": "console-production-lease",
    "x-sts2-lease-epoch": "1",
    "x-sts2-correlation-id": correlationId,
  };
}

async function allocateGatewayLease() {
  const requestBody = Buffer.from(JSON.stringify({
    instance_id: fixtureInfo.instance_id,
    caller_id: "harness",
    session_id: "console-production-gateway-session",
  }));
  const response = await gatewayHttpRequest(
    "POST",
    "/v1/sessions/allocate",
    requestBody,
  );
  if (response.status !== 200) {
    const errorCode = JSON.parse(response.body.toString("utf8"))?.error_code;
    throw new Error(`gateway_allocation_status_${response.status}_${errorCode || "unexpected"}`);
  }
  const value = JSON.parse(response.body.toString("utf8"));
  if (value.status !== "allocated"
    || value.instance_id !== fixtureInfo.instance_id
    || value.caller_id !== "harness"
    || value.session_id !== "console-production-gateway-session"
    || value.lease_id !== "console-production-lease"
    || value.lease_epoch !== 1) {
    throw new Error("gateway_allocation_identity_mismatch");
  }
  gatewayLeaseActive = true;
}

async function probeGatewayIdentity() {
  const requestPath = path.join(
    gatewayRoot,
    "protocol-artifact/runtime-v3-gameplay/golden/state-request.json",
  );
  const requestEnvelope = JSON.parse(fs.readFileSync(requestPath, "utf8"));
  requestEnvelope.correlation_id = "corr-console-production-probe";
  requestEnvelope.instance_id = fixtureInfo.instance_id;
  requestEnvelope.session_id = "console-production-gateway-session";
  requestEnvelope.lease_id = "console-production-lease";
  requestEnvelope.lease_epoch = 1;
  const body = Buffer.from(JSON.stringify(requestEnvelope));
  const response = await gatewayHttpRequest(
    "GET",
    `/v3/instances/${encodeURIComponent(fixtureInfo.instance_id)}/state`,
    body,
    gatewayLeaseHeaders(requestEnvelope.correlation_id),
  );
  if (response.status !== 200) {
    const errorCode = JSON.parse(response.body.toString("utf8"))?.error_code;
    throw new Error(`gateway_status_${response.status}_${errorCode || "unexpected"}`);
  }
  const envelope = JSON.parse(response.body.toString("utf8"));
  if (envelope.kind !== "state_response"
    || envelope.instance_id !== fixtureInfo.instance_id
    || envelope.session_id !== "console-production-gateway-session"
    || envelope.lease_id !== "console-production-lease"
    || envelope.correlation_id !== requestEnvelope.correlation_id
    || envelope.state_id !== "live:7"
    || envelope.generation !== 7) {
    throw new Error("gateway_identity_mismatch");
  }
}

function providerBridgeInvocationCount() {
  if (!bridgeCounterPath || !fs.existsSync(bridgeCounterPath)) return 0;
  return fs.readFileSync(bridgeCounterPath, "utf8").split("\n").filter(Boolean).length;
}

function syntheticModEffectCount() {
  const readOnlyPaths = new Set([
    "/api/v3/runtime/state",
    "/api/v3/runtime/legal-actions",
    "/api/v3/runtime/reobserve",
    "/api/v4/runtime/expert-state",
  ]);
  return modRequests.filter((request) => !readOnlyPaths.has(request.path)
    || request.method !== "GET").length;
}

function syntheticModEffectPaths() {
  const readOnlyPaths = new Set([
    "/api/v3/runtime/state",
    "/api/v3/runtime/legal-actions",
    "/api/v3/runtime/reobserve",
    "/api/v4/runtime/expert-state",
  ]);
  return modRequests.filter((request) => !readOnlyPaths.has(request.path)
    || request.method !== "GET")
    .map((request) => `${request.method} ${request.path}`);
}

function v3Response(template, kind, headers) {
  const value = JSON.parse(JSON.stringify(template));
  value.kind = kind;
  value.correlation_id = headers["x-sts2-correlation-id"] ?? "";
  value.instance_id = fixtureInfo.instance_id;
  value.session_id = "console-production-gateway-session";
  value.lease_id = "console-production-lease";
  value.lease_epoch = 1;
  value.generation = 7;
  value.state_id = "live:7";
  value.observation.state_id = "live:7";
  value.observation.generation = 7;
  value.observation.state = { state: "combat", turn_index: 8, enemies: [] };
  value.legal_actions = [{ action_id: "end:7", action: { kind: "end_turn" } }];
  if (kind === "legal_actions_response") value.observation = null;
  return value;
}

function startWorkflow() {
  const binary = binaryPath(
    "CONSOLE_POLICY_HARNESS_RUNTIME_BINARY",
    harnessRoot,
    "target/debug/sts2-harness-runtime",
  );
  return startChild(binary, ["serve-workflow"], serviceEnvironment, "serve-workflow.log", harnessRoot);
}

function startDemo() {
  const binary = binaryPath("CONTEXT_CONSOLE_BIN", appRoot, "target/debug/context-console");
  return startChild(binary, ["integrated-demo", "0"], limitedEnvironment(), "context-console.log", appRoot);
}

async function waitForPort(port, label, requestPath = "/") {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}${requestPath}`, {
        signal: AbortSignal.timeout(1000),
      });
      if (response.status < 500) return;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`${label} did not become ready within its bounded startup window`);
}

async function waitForGatewayIdentity() {
  const deadline = Date.now() + 15_000;
  let latestFailure = "not_attempted";
  while (Date.now() < deadline) {
    if (!gateway || gateway.spawnError || childExited(gateway)) {
      throw new Error("Gateway process exited before the identity probe succeeded");
    }
    try {
      if (!gatewayLeaseActive) await allocateGatewayLease();
      await probeGatewayIdentity();
      return;
    } catch (error) {
      latestFailure = error instanceof Error ? error.message : "unknown_probe_error";
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
  throw new Error(`Gateway did not return the owned fixture identity within its bounded startup window (${latestFailure})`);
}

async function releaseGatewayLease() {
  if (!gatewayLeaseActive || !gateway || childExited(gateway)) return;
  const response = await gatewayHttpRequest(
    "POST",
    `/v1/instances/${encodeURIComponent(fixtureInfo.instance_id)}/release`,
    Buffer.alloc(0),
    gatewayLeaseHeaders("corr-console-production-release"),
  );
  if (response.status !== 200) {
    throw new Error("owned Gateway lease release failed during fixture teardown");
  }
  gatewayLeaseActive = false;
}

async function waitForWorkflow() {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (!harness || harness.spawnError || childExited(harness)) {
      throw new Error("production workflow owner exited before readiness");
    }
    try {
      const response = await fetch(`http://127.0.0.1:${ownerPort}/v1/workflow-targets`, {
        headers: { authorization: `Bearer ${token}` },
        signal: AbortSignal.timeout(1000),
      });
      if (response.ok) {
        const catalog = await response.json();
        if (!catalog.targets?.some((target) => target.instance_id === fixtureInfo.instance_id)) {
          throw new Error("production target catalog does not contain the expected fixture identity");
        }
        return;
      }
    } catch (error) {
      if (error instanceof Error && error.message.includes("expected fixture identity")) throw error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("production workflow owner readiness timed out");
}

function proxyRequest(request, response, targetPort) {
  const headers = { ...request.headers, host: `127.0.0.1:${targetPort}` };
  delete headers.origin;
  const upstream = http.request({
    host: "127.0.0.1",
    port: targetPort,
    path: request.url,
    method: request.method,
    headers,
    timeout: 10_000,
  }, (upstreamResponse) => {
    response.writeHead(upstreamResponse.statusCode || 502, upstreamResponse.headers);
    upstreamResponse.pipe(response);
  });
  upstream.on("timeout", () => upstream.destroy(new Error("bounded local proxy timeout")));
  upstream.on("error", () => {
    if (!response.headersSent) response.writeHead(502, { "content-type": "application/json" });
    response.end('{"error":{"code":"local_owner_unavailable"}}');
  });
  request.pipe(upstream);
}

async function restartWorkflow(response) {
  await stopChild(harness);
  harness = undefined;
  const definitionPath = path.join(harnessRoot, "conformance/workflow-v1/valid-strict.json");
  const result = spawnSync(helper, [
    "verify",
    fixtureInfo.policy_store_path,
    definitionPath,
    fixtureInfo.source_path,
    fixtureInfo.target_path,
  ], {
    cwd: harnessRoot,
    encoding: "utf8",
    env: limitedEnvironment({ STS2_SERVED_PROVIDER_POLICY_KEY: providerKey }),
    maxBuffer: 2 * 1024 * 1024,
  });
  if (result.status !== 0) {
    response.writeHead(500, { "content-type": "application/json" });
    response.end('{"error":"durable policy owner reopen verification failed"}');
    return;
  }
  const line = result.stdout.trim().split("\n").at(-1);
  const evidence = line ? JSON.parse(line) : null;
  harness = startWorkflow();
  await waitForWorkflow();
  response.writeHead(200, { "content-type": "application/json" });
  response.end(JSON.stringify({ restarted: true, durable_owner: evidence }));
}

function startProxy() {
  proxyServer = http.createServer(async (request, response) => {
    if ((request.method === "GET" && request.url === "/__fixture")
      || (request.method === "POST" && request.url === "/__restart")) {
      if (request.headers.authorization !== `Bearer ${fixtureControlToken}`) {
        response.writeHead(401, { "content-type": "application/json" });
        response.end('{"error":"fixture_control_auth_required"}');
        return;
      }
    }
    if (request.method === "GET" && request.url === "/__fixture") {
      response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      response.end(JSON.stringify({
        run_id: fixtureInfo.run_id,
        request_id: fixtureInfo.request_id,
        instance_id: fixtureInfo.instance_id,
        definition_digest: fixtureInfo.definition_digest,
        context_source_digest: fixtureInfo.context_source_digest,
        expected_revision: fixtureInfo.baseline_revision,
        synthetic_mod_requests: modRequests.length,
        synthetic_mod_effects: syntheticModEffectCount(),
        synthetic_mod_effect_paths: syntheticModEffectPaths(),
        provider_bridge_invocations: providerBridgeInvocationCount(),
      }));
      return;
    }
    if (request.method === "POST" && request.url === "/__restart") {
      try { await restartWorkflow(response); }
      catch {
        if (!response.headersSent) response.writeHead(500, { "content-type": "application/json" });
        response.end('{"error":"production owner restart failed"}');
      }
      return;
    }
    const path = request.url?.split("?")[0];
    const isWorkflow = path?.startsWith("/v1/workflow-targets")
      || path === "/v1/context-bindings"
      || path === "/v1/context-bindings/bind"
      || path?.startsWith("/v1/workflow-runs");
    proxyRequest(request, response, isWorkflow ? ownerPort : demoPort);
  });
  return new Promise((resolve, reject) => {
    proxyServer.once("error", reject);
    proxyServer.listen(0, "127.0.0.1", () => {
      const address = proxyServer.address();
      if (!address || typeof address === "string") return reject(new Error("Console proxy did not bind"));
      proxyPort = address.port;
      resolve();
    });
  });
}

async function waitForDemoAddress(entry) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    if (entry.spawnError || childExited(entry)) throw new Error("Context Console demo exited before readiness");
    const match = entry.stdoutText.match(/integrated_demo_ready=http:\/\/(?:127\.0\.0\.1|localhost):(\d+)\/web\//);
    if (match) {
      demoPort = Number(match[1]);
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("Context Console demo did not announce a loopback address");
}

async function closeServer(server) {
  if (!server?.listening) return;
  const closed = new Promise((resolve) => server.close(resolve));
  server.closeAllConnections?.();
  await Promise.race([closed, new Promise((resolve) => setTimeout(resolve, 1000))]);
}

function emergencyCleanup() {
  for (const pid of children.keys()) {
    if (pid > 1) {
      try { process.kill(-pid, "SIGKILL"); } catch {}
    }
  }
  if (fixtureRoot) {
    try { fs.rmSync(fixtureRoot, { recursive: true, force: true }); } catch {}
  }
}

async function startProductionPolicyStack() {
  if (children.size !== 0) {
    throw new Error("the previous production fixture still owns a process group");
  }
  closing = false;
  shutdownPromise = undefined;
  modServer = undefined;
  proxyServer = undefined;
  fixtureRoot = undefined;
  fixtureInfo = undefined;
  helper = undefined;
  bridgeCounterPath = undefined;
  fixtureControlToken = undefined;
  harness = undefined;
  gateway = undefined;
  demo = undefined;
  serviceEnvironment = undefined;
  harnessRoot = undefined;
  gatewayRoot = undefined;
  mcpRoot = undefined;
  ownerPort = undefined;
  gatewayPort = undefined;
  modPort = undefined;
  demoPort = undefined;
  proxyPort = undefined;
  modRequests = [];
  allocatedPorts = new Set();
  gatewayLeaseActive = false;
  try {
    return await startProductionPolicyStackInner();
  } catch (error) {
    await closeCurrentStack();
    if (children.size !== 0 || modServer?.listening || proxyServer?.listening
      || (fixtureRoot && fs.existsSync(fixtureRoot))) {
      throw new Error("failed production fixture startup left owned processes or private files behind");
    }
    throw error;
  }
}

async function verifyFailedStartupCleanup() {
  const originalDemoBinary = process.env.CONTEXT_CONSOLE_BIN;
  process.env.CONTEXT_CONSOLE_BIN = path.join(appRoot, "target/debug/missing-context-console-fixture");
  let expectedStartupFailure = false;
  let startupErrorMessage = "none";
  try {
    await startProductionPolicyStack();
  } catch (error) {
    expectedStartupFailure = error instanceof Error
      && error.message === "CONTEXT_CONSOLE_BIN must point to the pinned production test binary";
    startupErrorMessage = error instanceof Error
      ? error.message.slice(0, 256)
      : "unknown";
  } finally {
    if (originalDemoBinary === undefined) delete process.env.CONTEXT_CONSOLE_BIN;
    else process.env.CONTEXT_CONSOLE_BIN = originalDemoBinary;
  }
  if (!expectedStartupFailure || children.size !== 0 || modServer?.listening || proxyServer?.listening
    || (fixtureRoot && fs.existsSync(fixtureRoot))) {
    throw new Error(`production fixture startup-failure cleanup regression failed: ${JSON.stringify({
      expected_startup_failure: expectedStartupFailure,
      startup_error: startupErrorMessage,
      owned_process_groups: children.size,
      mod_server_listening: Boolean(modServer?.listening),
      proxy_server_listening: Boolean(proxyServer?.listening),
      private_fixture_remains: Boolean(fixtureRoot && fs.existsSync(fixtureRoot)),
    })}`);
  }
}

async function startProductionPolicyStackInner() {
  if (closing) throw new Error("production fixture startup was interrupted");
  if (lock.repository !== "AI-Ascension/sts2-harness"
    || lock.gateway_repository !== "AI-Ascension/sts2-gateway"
    || lock.mcp_repository !== "AI-Ascension/sts2-mcp-server") {
    throw new Error("production fixture source lock is invalid");
  }
  harnessRoot = path.resolve(process.env.CONSOLE_POLICY_HARNESS_ROOT || "");
  gatewayRoot = path.resolve(process.env.CONSOLE_POLICY_GATEWAY_ROOT || "");
  mcpRoot = path.resolve(process.env.CONSOLE_POLICY_MCP_ROOT || "");
  if ([harnessRoot, gatewayRoot, mcpRoot].some((value) => value === path.resolve(""))) {
    throw new Error("production fixture requires the three pinned source checkout roots");
  }
  checkedOutRevision(harnessRoot, lock.revision, "Harness");
  checkedOutRevision(gatewayRoot, lock.gateway_revision, "Gateway");
  checkedOutRevision(mcpRoot, lock.mcp_revision, "MCP");
  const harnessBinary = binaryPath("CONSOLE_POLICY_HARNESS_RUNTIME_BINARY", harnessRoot, "target/debug/sts2-harness-runtime");
  const gatewayBinary = binaryPath("CONSOLE_POLICY_GATEWAY_BINARY", gatewayRoot, "target/debug/sts2-gateway-runtime");
  const mcpBinary = binaryPath("CONSOLE_POLICY_MCP_BINARY", mcpRoot, "target/debug/sts2-mcp-server");
  helper = process.env.CONSOLE_POLICY_FIXTURE_BINARY
    ? path.resolve(process.env.CONSOLE_POLICY_FIXTURE_BINARY)
    : path.join(appRoot, "tools/provider-policy-production-fixture/target/debug/console-provider-policy-production-fixture");
  if (!fs.statSync(helper, { throwIfNoEntry: false })?.isFile()) {
    throw new Error("CONSOLE_POLICY_FIXTURE_BINARY must point to the pinned production test binary");
  }
  fixtureRoot = privateDirectory(path.join(appRoot, "target"), "console-production-policy-");
  fixtureControlToken = crypto.randomBytes(32).toString("hex");
  ownerPort = await freePort(allocatedPorts);
  if (closing) throw new Error("production fixture startup was interrupted");
  gatewayPort = await freePort(allocatedPorts);
  if (closing) throw new Error("production fixture startup was interrupted");
  modPort = await freePort(allocatedPorts);
  if (closing) throw new Error("production fixture startup was interrupted");
  fixtureInfo = { ...bootstrapFixture(), ...(() => {
    const source = contextSourceFixture();
    return {
      context_source_document: source.document,
      context_source_bytes: source.sourceBytes,
      context_source_digest: source.digest,
    };
  })() };
  const bridgePath = path.join(fixtureRoot, "bounded-exo-bridge.sh");
  bridgeCounterPath = path.join(fixtureRoot, "provider-bridge-invocations.log");
  fs.writeFileSync(
    bridgePath,
    `#!/bin/sh\numask 077\nprintf 'call\\n' >> '${bridgeCounterPath}'\ncat >/dev/null\nprintf '%s' '{\"decision\":\"action\",\"action_id\":\"potion:7:potion:fire:enemy:1\",\"rationale\":\"use the visible potion\"}'\n`,
    { mode: 0o700 },
  );
  fs.chmodSync(bridgePath, 0o700);
  serviceEnvironment = serviceEnv();

  await startModServer();
  if (closing) throw new Error("production fixture startup was interrupted");
  gateway = startChild(gatewayBinary, [], limitedEnvironment({
    STS2_GATEWAY_ADDR: `127.0.0.1:${gatewayPort}`,
    STS2_MOD_ADDR: `127.0.0.1:${modPort}`,
    STS2_GATEWAY_TOKEN: "console-gateway-token",
    STS2_MOD_TOKEN: "console-mod-token",
    STS2_INSTANCE_ID: fixtureInfo.instance_id,
    STS2_CALLER_ID: "harness",
    STS2_SESSION_ID: "console-production-gateway-session",
    STS2_MCP_SESSION_ID: "console-production-mcp-session",
    STS2_LEASE_ID: "console-production-lease",
    STS2_LEASE_EPOCH: "1",
  }), "gateway.log", gatewayRoot);
  await waitForGatewayIdentity();
  if (closing) throw new Error("production fixture startup was interrupted");
  harness = startChild(harnessBinary, ["serve-workflow"], serviceEnvironment, "serve-workflow.log", harnessRoot);
  await waitForWorkflow();
  if (closing) throw new Error("production fixture startup was interrupted");
  demo = startDemo();
  await waitForDemoAddress(demo);
  await waitForPort(demoPort, "Context Console demo", "/web/");
  if (closing) throw new Error("production fixture startup was interrupted");
  await startProxy();

  return {
    baseUrl: `http://127.0.0.1:${proxyPort}`,
    browserEnvironment: {
      PATH: process.env.PATH || "/usr/bin:/bin",
      TMPDIR: fixtureRoot,
      ...(process.env.LD_LIBRARY_PATH ? { LD_LIBRARY_PATH: process.env.LD_LIBRARY_PATH } : {}),
      ...(process.env.FONTCONFIG_FILE ? { FONTCONFIG_FILE: process.env.FONTCONFIG_FILE } : {}),
    },
    fixtureControlHeaders: { authorization: `Bearer ${fixtureControlToken}` },
    sourcePolicyBytes: fs.readFileSync(fixtureInfo.source_path),
    targetPolicyBytes: fs.readFileSync(fixtureInfo.target_path),
    contextSourceDocument: fixtureInfo.context_source_document,
    contextSourceDigest: fixtureInfo.context_source_digest,
    close: closeCurrentStack,
  };
}

async function closeCurrentStack() {
  if (shutdownPromise) return shutdownPromise;
  closing = true;
  shutdownPromise = (async () => {
    let cleanupFailed = false;
    try {
      await Promise.all([closeServer(proxyServer), closeServer(modServer)]).catch(() => {
        cleanupFailed = true;
      });
      await releaseGatewayLease().catch(() => {
        cleanupFailed = true;
      });
      const stopped = await Promise.allSettled([...children.values()].map(stopChild));
      cleanupFailed ||= stopped.some((result) => result.status === "rejected");
    } finally {
      emergencyCleanup();
      if (fixtureRoot) fs.rmSync(fixtureRoot, { recursive: true, force: true });
    }
    if (cleanupFailed) throw new Error("production fixture teardown failed to release an owned resource");
  })();
  return shutdownPromise;
}

process.on("exit", emergencyCleanup);

module.exports = {
  closeCurrentStack,
  startProductionPolicyStack,
  verifyFailedStartupCleanup,
};
