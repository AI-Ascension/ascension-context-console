// SPDX-License-Identifier: MIT

const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { chromium, firefox, webkit } = require("playwright");
const {
  closeCurrentStack,
  startProductionPolicyStack,
  verifyFailedStartupCleanup,
} = require("./production-policy-stack.cjs");

const appRoot = path.resolve(__dirname, "../..");
const harnessRoot = path.resolve(process.env.CONSOLE_POLICY_HARNESS_ROOT || "");
let activeStack;
let activeBrowser;
let activeBrowserLaunch;
let terminationStarted = false;
const ownerToken = "console-live-ci-token";
const proposalId = "migration.console.production";
const approvalReference = "approval.console.production";

async function terminate(signal, exitCode) {
  if (terminationStarted) return;
  terminationStarted = true;
  try {
    if (activeBrowserLaunch) {
      const launchingBrowser = await activeBrowserLaunch.catch(() => undefined);
      await launchingBrowser?.close().catch(() => {});
    }
    await activeBrowser?.close().catch(() => {});
    await activeStack?.close();
    await closeCurrentStack();
  } catch {}
  process.exit(exitCode);
}

process.on("SIGINT", () => { void terminate("SIGINT", 130); });
process.on("SIGTERM", () => { void terminate("SIGTERM", 143); });

function digest(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function canonicalJson(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const entries = Object.entries(value).sort(([left], [right]) => left.localeCompare(right));
  return `{${entries.map(([key, entry]) => `${JSON.stringify(key)}:${canonicalJson(entry)}`).join(",")}}`;
}

function servedDefinition() {
  const definition = JSON.parse(fs.readFileSync(
    path.join(harnessRoot, "conformance/workflow-v1/valid-strict.json"),
    "utf8",
  ));
  definition.annotations.synthetic = false;
  definition.game_profile = "sts2-live-v1";
  definition.policy_ref = "policy.live.v1";
  definition.graphs[0].nodes[0].config.projection_ref = "fair-play.live.v1";
  definition.graphs[0].nodes[1].config.decision_profile_ref = "decision.live.v1";
  definition.graphs[0].nodes[1].config.context_ref = "context.live.v1";
  definition.capabilities.required[0] = "observe.fair-play.v1";
  return definition;
}

async function waitForText(page, selector, expression, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const text = await page.locator(selector).textContent().catch(() => "");
    if (expression.test(text || "")) return text;
    await page.waitForTimeout(50);
  }
  throw new Error("Console did not reach the expected saved-policy state");
}

function commandResponse(response, operation) {
  assert.equal(response.status(), 200, `${operation} must reach the production owner successfully`);
  return response.json();
}

async function submitAdmission(page, baseUrl, fixture) {
  const definition = servedDefinition();
  const definitionDigest = digest(Buffer.from(canonicalJson(definition)));
  assert.equal(definitionDigest, fixture.definition_digest, "browser definition must match the prebound owner scope");

  const result = await page.evaluate(async ({ definition, definitionDigest, fixture, token }) => {
    const headers = {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    };
    const catalogResponse = await fetch("/v1/workflow-targets", { headers, cache: "no-store" });
    const catalog = await catalogResponse.json();
    if (!catalogResponse.ok || catalog.schema_version !== "ascension.workflow-targets/v1") {
      return { phase: "catalog", status: catalogResponse.status };
    }
    const descriptor = catalog.targets.find((target) => target.instance_id === fixture.instance_id);
    if (!descriptor) return { phase: "catalog-identity", status: 404 };
    const target = {
      instance_id: descriptor.instance_id,
      execution_profile: "live.workflow.v1",
      execution_mode: "live",
      workflow_revision: definition.version,
      compatibility_revision: descriptor.compatibility_revision,
      capability_revision: descriptor.capability_revision,
      game_profile: "sts2-live-v1",
      save_profile: null,
      inference_profile: null,
      context_capability: null,
      provider_capability: null,
    };
    const preflightResponse = await fetch("/v1/workflow-targets/preflight", {
      method: "POST",
      headers,
      cache: "no-store",
      body: JSON.stringify({
        schema_version: "ascension.workflow-admission/v1",
        request_id: fixture.request_id,
        workflow_definition_digest: definitionDigest,
        target,
      }),
    });
    const preflight = await preflightResponse.json();
    if (!preflightResponse.ok) return { phase: "preflight", status: preflightResponse.status };
    const submissionResponse = await fetch("/v1/workflow-runs", {
      method: "POST",
      headers,
      cache: "no-store",
      body: JSON.stringify({
        schema_version: "ascension.management/v1",
        request_id: fixture.request_id,
        definition,
        artifact_id: null,
        instance_id: fixture.instance_id,
        profile: "live.workflow.v1",
        admission: preflight.admission,
      }),
    });
    const run = await submissionResponse.json();
    return {
      phase: "submission",
      status: submissionResponse.status,
      runId: run.workflow_run_id,
      errorCode: run?.error?.code ?? null,
    };
  }, { definition, definitionDigest, fixture, token: ownerToken });
  assert.equal(result.phase, "submission", "target catalog and admission must be served by Harness");
  assert.equal(result.status, 200, `prebound production run admission failed: ${result.errorCode || "unknown"}`);
  assert.equal(result.runId, fixture.run_id);
}

async function resolveContextOwnerAndRecoverReceipt(page, stack, fixture) {
  const result = await page.evaluate(async ({ fixture, token, source }) => {
    const headers = {
      Authorization: `Bearer ${token}`,
      Accept: "application/json",
    };
    const json = async (path, options = {}) => {
      const response = await fetch(path, {
        ...options,
        headers: { ...headers, ...(options.headers || {}) },
        cache: "no-store",
      });
      const value = await response.json();
      return { response, value };
    };
    const status = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}`);
    if (!status.response.ok) return { phase: "status", status: status.response.status };
    const step = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}/commands`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema_version: "ascension.management/v1",
        command_id: "console-context-owner-observe",
        run_id: fixture.run_id,
        expected_revision: status.value.run.run_revision,
        actor_scope: "profile:console-live",
        kind: "step",
        parameters: {},
      }),
    });
    if (!step.response.ok) return { phase: "step", status: step.response.status, value: step.value };
    const current = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}`);
    if (current.value.run?.cursor?.node_id !== "decide") {
      return { phase: "cursor", status: current.response.status, value: current.value };
    }
    const catalog = await json("/v1/context-bindings");
    const descriptor = catalog.value.descriptors?.find((candidate) =>
      candidate.context_ref === "context.live.v1" && candidate.node_kinds?.includes("decide"));
    if (!descriptor) {
      return {
        phase: "catalog",
        status: catalog.response.status,
        value: {
          schema_version: catalog.value?.schema_version ?? null,
          owner_id: catalog.value?.owner_id ?? null,
          descriptors: Array.isArray(catalog.value?.descriptors)
            ? catalog.value.descriptors.map((candidate) => ({
              context_ref: candidate.context_ref ?? null,
              node_kinds: candidate.node_kinds ?? null,
            }))
            : null,
          error: catalog.value?.error ?? null,
        },
      };
    }
    const cursor = current.value.run.cursor;
    const bindingRequest = {
      workflow_run_id: fixture.run_id,
      definition_digest: current.value.run.definition_digest,
      instance_id: fixture.instance_id,
      graph_id: cursor.graph_id,
      node_id: cursor.node_id,
      node_execution_id: cursor.node_execution_id,
      node_kind: "decide",
      context_ref: "context.live.v1",
      binding_id: descriptor.binding_id,
      binding_version: descriptor.version,
      binding_digest: descriptor.digest,
    };
    const bound = await json("/v1/context-bindings/bind", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(bindingRequest),
    });
    if (!bound.response.ok) return { phase: "bind", status: bound.response.status, value: bound.value };
    const association = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}/context-owner-association`);
    const limits = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}/context-owner-effective-limits`);
    if (!association.response.ok || !limits.response.ok) {
      return { phase: "owner-read", status: association.response.status, value: association.value };
    }
    const upload = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}/context-sources/strategy`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema_version: "ascension.context-owner.context-source-upload.v1",
        document: source,
      }),
    });
    if (!upload.response.ok) return { phase: "upload", status: upload.response.status, value: upload.value };
    const sourceStatus = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}/context-owner-source-status`);
    const adoptionCommand = {
      schema_version: "ascension.context-owner.context-source-adoption.v1",
      idempotency_key: "console-context-owner-receipt.1",
      expected_control_version: sourceStatus.value.boundary.control_version,
      expected_revision_id: sourceStatus.value.active_revision_id,
      expected_boundary: sourceStatus.value.boundary,
    };
    const adopted = await json(`/v1/workflow-runs/${encodeURIComponent(fixture.run_id)}/context-sources/strategy/adopt`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(adoptionCommand),
    });
    if (!adopted.response.ok) return { phase: "adopt", status: adopted.response.status, value: adopted.value };
    return {
      phase: "ready",
      catalog_owner_id: catalog.value.owner_id,
      catalog_binding_id: descriptor.binding_id,
      association: association.value,
      limits: limits.value,
      receipt: adopted.value,
      command: { commit: {
        idempotency_key: adoptionCommand.idempotency_key,
        expected_control_version: adoptionCommand.expected_control_version,
        expected_revision_id: adoptionCommand.expected_revision_id,
        expected_boundary: adoptionCommand.expected_boundary,
        preview_manifest_digest: fixture.context_source_digest,
        approved_manifest_digest: fixture.context_source_digest,
      } },
    };
  }, {
    fixture,
    token: ownerToken,
    source: stack.contextSourceDocument,
  });
  assert.equal(result.phase, "ready", `context owner journey failed at ${result.phase}: ${JSON.stringify(result.value)}`);
  assert.equal(result.catalog_owner_id, "console-served-context-owner");
  assert.equal(result.catalog_binding_id, "console-served-context-owner.decide.v1");
  assert.equal(result.association.binding.workflow_run_id, fixture.run_id);
  assert.equal(result.association.binding.binding_id, result.limits.binding_id);
  assert.equal(result.receipt.idempotency_key, result.command.commit.idempotency_key);

  await page.locator("#context-owner-run-id").fill(fixture.run_id);
  await page.locator("#context-owner-token").fill(ownerToken);
  await page.getByRole("button", { name: "Refresh current owner", exact: true }).click();
  await waitForText(page, "#context-owner-association", new RegExp(fixture.run_id));
  await waitForText(page, "#context-owner-limits", /Maximum items|Items/);
  await page.getByLabel("Typed receipt lookup command", { exact: true }).fill(JSON.stringify(result.command));
  await page.getByRole("button", { name: "Recover receipt", exact: true }).click();
  await waitForText(page, "#context-owner-receipt", new RegExp(result.receipt.command_id));
  return result;
}

async function authNegativeChecks(page, runId) {
  const checks = await page.evaluate(async ({ runId, token }) => {
    const pathFor = (id) =>
      `/v1/workflow-runs/${encodeURIComponent(id)}/provider-session-policy`;
    const noToken = await fetch(pathFor(runId), { cache: "no-store" });
    const badToken = await fetch(pathFor(runId), {
      headers: { Authorization: "Bearer invalid-console-fixture-token" },
      cache: "no-store",
    });
    const deniedWrite = await fetch(`${pathFor(runId)}/import?expected_revision=3`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    const foreignRun = await fetch(pathFor("run.live.foreign-owner"), {
      headers: { Authorization: `Bearer ${token}` },
      cache: "no-store",
    });
    const foreignBody = await foreignRun.json();
    return {
      noToken: noToken.status,
      badToken: badToken.status,
      deniedWrite: deniedWrite.status,
      foreignRunStatus: foreignRun.status,
      foreignRunError: foreignBody?.error?.code ?? null,
    };
  }, { runId, token: ownerToken });
  assert.deepEqual(
    [checks.noToken, checks.badToken, checks.deniedWrite],
    [401, 401, 401],
    "production policy routes must reject missing or invalid bearer credentials",
  );
  assert.notEqual(checks.foreignRunStatus, 200, "valid credentials must not expose another run's policy owner");
  assert.equal(checks.foreignRunError, "run_not_found");
}

async function upload(page, selector, name, bytes, buttonName, routeSuffix, operation) {
  await page.getByLabel(selector).setInputFiles({
    name,
    mimeType: "application/json",
    buffer: bytes,
  });
  const responsePromise = page.waitForResponse((response) =>
    response.request().method() === "POST"
    && new URL(response.url()).pathname.endsWith(routeSuffix));
  await page.getByRole("button", { name: buttonName, exact: true }).click();
  const response = await responsePromise;
  assert.deepEqual(
    Buffer.from(response.request().postDataBuffer() || []),
    bytes,
    `${operation} must preserve the uploaded file bytes on the actual UI request`,
  );
  return commandResponse(response, operation);
}

async function auditBrowser(browserType, name) {
  let stack;
  let browser;
  try {
    stack = await startProductionPolicyStack();
    activeStack = stack;
    const launchOptions = name === "Chromium"
      ? { headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage"] }
      : { headless: true };
    launchOptions.env = stack.browserEnvironment;
    activeBrowserLaunch = browserType.launch(launchOptions);
    browser = await activeBrowserLaunch;
    activeBrowserLaunch = undefined;
    activeBrowser = browser;
    const context = await browser.newContext({
      viewport: { width: 1440, height: 1000 },
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    const ownerRequests = [];
    const contextRequests = [];
    page.on("request", (request) => {
      const url = new URL(request.url());
      if (url.pathname.includes("/provider-session-policy")) {
        ownerRequests.push({
          method: request.method(),
          pathname: url.pathname,
          search: url.search,
          body: request.postDataBuffer(),
          headers: request.headers(),
          url: request.url(),
        });
      }
      if (url.pathname.includes("/context-owner-association")
        || url.pathname.includes("/context-owner-effective-limits")
        || url.pathname.includes("/context-control-receipts/lookup")) {
        contextRequests.push({ method: request.method(), pathname: url.pathname, url: request.url() });
      }
    });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.name));
    await page.goto(`${stack.baseUrl}/web/`, { waitUntil: "networkidle", timeout: 30_000 });
    await page.waitForSelector("#policy-owner-panel");

    const untrustedFixtureControl = await fetch(`${stack.baseUrl}/__fixture`, {
      signal: AbortSignal.timeout(3000),
    });
    assert.equal(untrustedFixtureControl.status, 401);
    const fixtureResponse = await fetch(`${stack.baseUrl}/__fixture`, {
      headers: stack.fixtureControlHeaders,
      signal: AbortSignal.timeout(3000),
    });
    assert.equal(fixtureResponse.status, 200);
    const fixture = await fixtureResponse.json();
    assert.equal(fixture.expected_revision, 3);
    await submitAdmission(page, stack.baseUrl, fixture);
    await authNegativeChecks(page, fixture.run_id);
    const contextOwnerResult = await resolveContextOwnerAndRecoverReceipt(page, stack, fixture);

    await page.locator("#policy-owner-run-id").fill(fixture.run_id);
    await page.locator("#policy-owner-token").fill(ownerToken);
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-revision", /owner revision 3/);
    const panel = page.locator("#policy-owner-panel");
    assert.match(await panel.locator("#policy-owner-active").textContent(), /console-production-baseline@1/);
    assert.equal(await page.locator("#policy-owner-content").isHidden(), false);

    const sourceBytes = stack.sourcePolicyBytes;
    const targetBytes = stack.targetPolicyBytes;
    const sourceSha = digest(sourceBytes);
    const targetSha = digest(targetBytes);
    const stalePolicy = JSON.parse(sourceBytes.toString("utf8"));
    stalePolicy.policy_id = "console-stale-policy";
    stalePolicy.version = 3;
    stalePolicy.epoch = 3;
    const staleBytes = Buffer.from(JSON.stringify(stalePolicy, null, 2));

    const firstImport = await upload(
      page,
      "Policy JSON file (maximum 1 MiB)",
      "migration-source.json",
      sourceBytes,
      "Import exact file bytes",
      "/provider-session-policy/import",
      "import",
    );
    assert.equal(firstImport.policy_sha256, sourceSha);
    assert.equal(firstImport.revision, 4);
    await waitForText(page, "#policy-owner-revision", /owner revision 4/);

    const duplicateImport = await upload(
      page,
      "Policy JSON file (maximum 1 MiB)",
      "migration-source.json",
      sourceBytes,
      "Import exact file bytes",
      "/provider-session-policy/import",
      "import",
    );
    assert.equal(duplicateImport.policy_sha256, sourceSha);
    assert.equal(duplicateImport.revision, 4, "duplicate exact-byte import must not add a history revision");
    await waitForText(page, "#policy-owner-revision", /owner revision 4/);

    await page.getByLabel("Saved source policy").selectOption(sourceSha);
    await page.getByLabel("Proposal ID").fill(proposalId);
    const proposalResult = await upload(
      page,
      "Target policy JSON (maximum 1 MiB)",
      "migration-target.json",
      targetBytes,
      "Create migration proposal",
      `/proposals/${proposalId}`,
      "propose",
    );
    assert.match(proposalResult.proposal_sha256, /^[a-f0-9]{64}$/);
    assert.equal(proposalResult.revision, 5);
    await waitForText(page, "#policy-owner-revision", /owner revision 5/);

    const approvalInput = page.getByLabel(`Approval reference for ${proposalId}`);
    await approvalInput.fill(approvalReference);
    const approvalResponsePromise = page.waitForResponse((response) =>
      response.request().method() === "POST"
      && new URL(response.url()).pathname.endsWith("/approve"));
    await page.getByRole("button", { name: "Record approval", exact: true }).click();
    const approvalResponse = await approvalResponsePromise;
    const approvalResult = await commandResponse(approvalResponse, "approve");
    assert.equal(approvalResult.revision, 6);
    await waitForText(page, "#policy-owner-revision", /owner revision 6/);
    assert.match(await page.locator("#policy-owner-proposals").textContent(), /Approval recorded by owner/);

    await page.getByLabel(`Approval reference for ${proposalId}`).fill(approvalReference);
    const adoptionResponsePromise = page.waitForResponse((response) =>
      response.request().method() === "POST"
      && new URL(response.url()).pathname.endsWith("/adopt"));
    await page.getByRole("button", { name: "Adopt approved proposal", exact: true }).click();
    const adoptionResponse = await adoptionResponsePromise;
    const adoptionResult = await commandResponse(adoptionResponse, "adopt");
    assert.equal(adoptionResult.revision, 7);
    assert.equal(adoptionResult.policy_sha256, targetSha);
    await waitForText(page, "#policy-owner-revision", /owner revision 7/);
    await waitForText(page, "#policy-owner-active", new RegExp(targetSha));
    const history = await page.locator("#policy-owner-history").textContent();
    assert.ok(history.includes(sourceSha), "Console history must retain the original source digest");
    assert.ok(history.includes(targetSha), "Console history must retain the adopted target digest");
    assert.equal(await page.locator("#policy-owner-history li").count(), 3);
    assert.match(await page.locator("#policy-owner-proposals").textContent(), /adopted/);

    const staleWrite = await page.evaluate(async ({ runId, token, bytes }) => {
      const response = await fetch(
        `/v1/workflow-runs/${encodeURIComponent(runId)}/provider-session-policy/import?expected_revision=6`,
        {
          method: "POST",
          headers: {
            Authorization: `Bearer ${token}`,
            "Content-Type": "application/json",
          },
          body: new Uint8Array(bytes),
          cache: "no-store",
        },
      );
      const viewResponse = await fetch(
        `/v1/workflow-runs/${encodeURIComponent(runId)}/provider-session-policy`,
        { headers: { Authorization: `Bearer ${token}` }, cache: "no-store" },
      );
      return { status: response.status, view: await viewResponse.json() };
    }, { runId: fixture.run_id, token: ownerToken, bytes: [...staleBytes] });
    assert.equal(staleWrite.status, 409, "stale expected revision must be rejected by the production owner");
    assert.equal(staleWrite.view.value.revision, 7);
    assert.ok(
      ownerRequests.some((entry) => entry.search.includes("expected_revision=6")
        && Buffer.from(entry.body || []).equals(staleBytes)),
      "the stale-CAS check must submit a distinct valid policy body with the stale revision",
    );

    await page.locator("#policy-owner-run-id").fill("run.live.foreign-owner");
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-message", /Could not load|not found|unavailable/i);
    assert.equal(await page.locator("#policy-owner-content").isHidden(), true);
    await page.locator("#policy-owner-run-id").fill(fixture.run_id);
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-revision", /owner revision 7/);
    await waitForText(page, "#policy-owner-active", new RegExp(targetSha));

    await page.getByRole("button", { name: "Clear credentials", exact: true }).click();
    assert.equal(await page.locator("#policy-owner-token").inputValue(), "");
    assert.equal(await page.locator("#policy-owner-run-id").inputValue(), "");
    assert.equal(await page.locator("#policy-owner-content").isHidden(), true);
    await page.locator("#policy-owner-run-id").fill(fixture.run_id);
    await page.locator("#policy-owner-token").fill(ownerToken);
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-revision", /owner revision 7/);

    const restartResponse = await fetch(`${stack.baseUrl}/__restart`, {
      method: "POST",
      headers: stack.fixtureControlHeaders,
      signal: AbortSignal.timeout(10_000),
    });
    assert.equal(restartResponse.status, 200);
    const restart = await restartResponse.json();
    assert.deepEqual(restart, {
      restarted: true,
      durable_owner: {
        reopened: true,
        run_id: fixture.run_id,
        revision: 7,
        history_count: 3,
        proposal_state: "adopted",
        active_policy_id: "console-production-migration",
        source_policy_sha256: sourceSha,
        target_policy_sha256: targetSha,
        effect_class: "local_metadata_only",
        inference_calls: 0,
        game_effects: 0,
      },
    });
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-revision", /owner revision 7/);
    await waitForText(page, "#policy-owner-active", new RegExp(targetSha));
    assert.equal(await page.locator("#policy-owner-history li").count(), 3);
    await page.getByRole("button", { name: "Refresh current owner", exact: true }).click();
    await waitForText(page, "#context-owner-message", /Could not load the current owner|unavailable/);
    assert.equal(await page.locator("#context-owner-association").textContent(), "", "restart must clear the stale current association");
    await page.getByLabel("Typed receipt lookup command", { exact: true }).fill(JSON.stringify(contextOwnerResult.command));
    await page.getByRole("button", { name: "Recover receipt", exact: true }).click();
    await waitForText(page, "#context-owner-receipt", new RegExp(contextOwnerResult.receipt.command_id));
    assert.deepEqual(
      JSON.parse(await page.locator("#context-owner-receipt").textContent()),
      contextOwnerResult.receipt,
      "Console must render the exact durable receipt recovered after restart",
    );

    let successStartedResolve;
    const successStarted = new Promise((resolve) => { successStartedResolve = resolve; });
    let successReleaseResolve;
    const successRelease = new Promise((resolve) => { successReleaseResolve = resolve; });
    let successCompletedResolve;
    const successCompleted = new Promise((resolve) => { successCompletedResolve = resolve; });
    let failureStartedResolve;
    const failureStarted = new Promise((resolve) => { failureStartedResolve = resolve; });
    let failureReleaseResolve;
    const failureRelease = new Promise((resolve) => { failureReleaseResolve = resolve; });
    let failureCompletedResolve;
    const failureCompleted = new Promise((resolve) => { failureCompletedResolve = resolve; });
    let recoveryAttempt = 0;
    await page.route("**/context-control-receipts/lookup", async (route) => {
      recoveryAttempt += 1;
      if (recoveryAttempt === 1) {
        successStartedResolve();
        const response = await route.fetch();
        await successRelease;
        await route.fulfill({ response });
        successCompletedResolve();
      } else {
        failureStartedResolve();
        await failureRelease;
        await route.abort("failed");
        failureCompletedResolve();
      }
    });
    const isReceiptRequest = (request) =>
      request.method() === "POST"
      && new URL(request.url()).pathname.endsWith("/context-control-receipts/lookup");
    const successRequestFinished = page.waitForEvent("requestfinished", { predicate: isReceiptRequest });
    await page.getByRole("button", { name: "Recover receipt", exact: true }).click();
    await successStarted;
    await page.locator("#context-owner-run-id").fill("run.live.foreign-owner");
    successReleaseResolve();
    await Promise.all([successCompleted, successRequestFinished]);
    await page.waitForTimeout(0);
    assert.equal(
      (await page.locator("#context-owner-receipt").textContent()).trim(),
      "No historical receipt recovered.",
      "a delayed receipt success must not repopulate after the run selection changes",
    );
    await page.locator("#context-owner-run-id").fill(fixture.run_id);
    const failedRequest = page.waitForEvent("requestfailed", { predicate: isReceiptRequest });
    await page.getByRole("button", { name: "Recover receipt", exact: true }).click();
    await failureStarted;
    await page.locator("#context-owner-run-id").fill("run.live.foreign-owner");
    failureReleaseResolve();
    await Promise.all([failureCompleted, failedRequest]);
    await page.waitForTimeout(0);
    assert.equal(
      (await page.locator("#context-owner-receipt").textContent()).trim(),
      "No historical receipt recovered.",
      "a delayed receipt error must not overwrite the cleared selection",
    );
    await page.unroute("**/context-control-receipts/lookup");
    assert.ok(
      contextRequests.some((entry) => entry.pathname.endsWith("/context-owner-association")),
      "Console must read the actual current context-owner association route",
    );
    assert.ok(
      contextRequests.some((entry) => entry.pathname.endsWith("/context-owner-effective-limits")),
      "Console must read the actual effective-limits route",
    );
    assert.ok(
      contextRequests.some((entry) => entry.method === "POST" && entry.pathname.endsWith("/context-control-receipts/lookup")),
      "Console must recover the durable receipt through the actual typed lookup route",
    );

    const policyCalls = ownerRequests.filter((entry) => entry.pathname.includes("/provider-session-policy"));
    assert.ok(policyCalls.some((entry) => entry.method === "GET"), "Console must read the actual current owner route");
    for (const method of ["POST"]) {
      assert.ok(policyCalls.some((entry) => entry.method === method), `Console must submit actual ${method} commands`);
    }
    assert.ok(
      policyCalls.some((entry) => entry.pathname.endsWith("/import") && Buffer.from(entry.body || []).equals(sourceBytes)),
      "the real Console import must submit the exact source bytes",
    );
    assert.ok(
      policyCalls.some((entry) => entry.pathname.endsWith(`/proposals/${proposalId}`)
        && Buffer.from(entry.body || []).equals(targetBytes)),
      "the real Console proposal must submit the exact target bytes",
    );
    assert.ok(
      policyCalls.every((entry) => !entry.url.includes(ownerToken) && !entry.url.includes(approvalReference)),
      "bearer and approval references must not be placed in policy URLs",
    );
    const runtimeEvidenceResponse = await fetch(`${stack.baseUrl}/__fixture`, {
      headers: stack.fixtureControlHeaders,
      signal: AbortSignal.timeout(3000),
    });
    assert.equal(runtimeEvidenceResponse.status, 200);
    const runtimeEvidence = await runtimeEvidenceResponse.json();
    assert.ok(runtimeEvidence.synthetic_mod_requests > 0, "the pinned Gateway must reach the synthetic Mod fixture");
    assert.equal(
      runtimeEvidence.synthetic_mod_effects,
      0,
      `the Mod fixture must receive no gameplay-mutating routes (${runtimeEvidence.synthetic_mod_effect_paths.join(", ")})`,
    );
    assert.equal(runtimeEvidence.provider_bridge_invocations, 0, "the provider bridge must not be invoked");
    assert.ok(pageErrors.length === 0, "the Console browser journey must not produce uncaught page errors");

    await context.close();
    process.stdout.write(`production served policy journey passed (${name})\n`);
  } finally {
    if (browser) await browser.close().catch(() => {});
    if (activeBrowser === browser) activeBrowser = undefined;
    if (stack) await stack.close();
    if (activeStack === stack) activeStack = undefined;
  }
}

async function main() {
  await verifyFailedStartupCleanup();
  process.stdout.write("production fixture startup-failure cleanup passed\n");
  const available = [
    [chromium, "Chromium"],
    [firefox, "Firefox"],
    [webkit, "WebKit"],
  ];
  const requested = process.env.CONSOLE_POLICY_BROWSER_PROJECTS
    ?.split(",")
    .map((entry) => entry.trim().toLowerCase())
    .filter(Boolean);
  const engines = requested
    ? available.filter(([, name]) => requested.includes(name.toLowerCase()))
    : available;
  if (engines.length === 0 || (requested && engines.length !== new Set(requested).size)) {
    throw new Error("CONSOLE_POLICY_BROWSER_PROJECTS contains an unsupported browser name");
  }
  for (const [engine, name] of engines) await auditBrowser(engine, name);
}

main().catch((error) => {
  process.stderr.write(`production saved-policy browser audit failed: ${error instanceof Error ? error.message : "unknown fixture error"}\n`);
  process.exitCode = 1;
});
