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

    await page.getByLabel("Workflow run ID").fill(fixture.run_id);
    await page.getByLabel("Workflow owner bearer token").fill(ownerToken);
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

    await page.getByLabel("Workflow run ID").fill("run.live.foreign-owner");
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-message", /Could not load|not found|unavailable/i);
    assert.equal(await page.locator("#policy-owner-content").isHidden(), true);
    await page.getByLabel("Workflow run ID").fill(fixture.run_id);
    await page.getByRole("button", { name: "Load owner history", exact: true }).click();
    await waitForText(page, "#policy-owner-revision", /owner revision 7/);
    await waitForText(page, "#policy-owner-active", new RegExp(targetSha));

    await page.getByRole("button", { name: "Clear credentials", exact: true }).click();
    assert.equal(await page.getByLabel("Workflow owner bearer token").inputValue(), "");
    assert.equal(await page.getByLabel("Workflow run ID").inputValue(), "");
    assert.equal(await page.locator("#policy-owner-content").isHidden(), true);
    await page.getByLabel("Workflow run ID").fill(fixture.run_id);
    await page.getByLabel("Workflow owner bearer token").fill(ownerToken);
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
