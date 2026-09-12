// SPDX-License-Identifier: MIT

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn, execFileSync } = require('node:child_process');

const root = path.resolve(__dirname, '..', '..');
const playwrightModule = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium } = require(playwrightModule);
const playwrightVersion = require(`${playwrightModule}/package.json`).version;
const prebuiltBinary = process.env.CONTEXT_CONSOLE_BIN ? path.resolve(process.env.CONTEXT_CONSOLE_BIN) : undefined;
// Historical evidence is immutable. CI must supply its own disposable directory.
const evidenceDir = process.env.CONTEXT_BROWSER_AUDIT_OUT
  ? path.resolve(process.env.CONTEXT_BROWSER_AUDIT_OUT)
  : fs.mkdtempSync(path.join(os.tmpdir(), 'context-browser-audit-'));
fs.mkdirSync(evidenceDir, { recursive: true });
const auditCommand = [
  `FONTCONFIG_PATH=${process.env.FONTCONFIG_PATH || '<unset>'}`,
  `FONTCONFIG_FILE=${process.env.FONTCONFIG_FILE || '<unset>'}`,
  `XDG_DATA_DIRS=${process.env.XDG_DATA_DIRS || '<unset>'}`,
  `LD_LIBRARY_PATH=${process.env.LD_LIBRARY_PATH || '<unset>'}`,
  `PLAYWRIGHT_MODULE=playwright@${playwrightVersion}`,
  'PLAYWRIGHT_BROWSERS_PATH=<cached-browser>',
  'node tools/browser-audit/integrated_browser_audit.cjs',
].join(' ');

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function waitForServer(child) {
  return new Promise((resolve, reject) => {
    let output = '';
    const timeout = setTimeout(() => reject(new Error('integrated demo did not become ready within 30 seconds')), 30_000);
    const onData = (chunk) => {
      output += chunk.toString();
      const line = output.split(/\r?\n/).find((value) => value.startsWith('integrated_demo_ready='));
      if (line) {
        clearTimeout(timeout);
        child.stdout.off('data', onData);
        resolve(line.slice('integrated_demo_ready='.length).trim());
      }
    };
    child.stdout.on('data', onData);
    child.once('error', (error) => { clearTimeout(timeout); reject(error); });
    child.once('exit', (code, signal) => { clearTimeout(timeout); reject(new Error(`integrated demo exited before readiness: ${code}/${signal}`)); });
  });
}

async function stopServer(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill('SIGTERM');
  await Promise.race([
    new Promise((resolve) => child.once('exit', resolve)),
    new Promise((resolve) => setTimeout(resolve, 5_000)),
  ]);
  if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
}

async function run() {
  const child = spawn(
    prebuiltBinary || 'cargo',
    prebuiltBinary
      ? ['integrated-demo', '0']
      : ['run', '--locked', '--package', 'context-service', '--bin', 'context-console', '--', 'integrated-demo', '0'],
    {
      cwd: root,
      env: process.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  let serverStderr = '';
  child.stderr.on('data', (chunk) => { serverStderr += chunk.toString(); });
  let browser;
  try {
    const readyUrl = await waitForServer(child);
    const address = new URL(readyUrl);
    assert.equal(address.protocol, 'http:');
    assert.ok(['127.0.0.1', 'localhost', '[::1]'].includes(address.hostname), 'demo must bind loopback');
    const base = address.origin;
    browser = await chromium.launch({
      headless: true,
      args: ['--no-sandbox', '--disable-dev-shm-usage'],
    });
    const context = await browser.newContext({
      baseURL: base,
      reducedMotion: 'reduce',
      viewport: { width: 1440, height: 1000 },
    });
    await context.route('**/*', (route) => new URL(route.request().url()).origin === base ? route.continue() : route.abort());
    const page = await context.newPage();
    const requests = [];
    const consoleErrors = [];
    const pageErrors = [];
    page.on('request', (request) => requests.push(request.url()));
    page.on('console', (message) => { if (message.type() === 'error') consoleErrors.push(message.text()); });
    page.on('pageerror', (error) => pageErrors.push(String(error)));
    await page.goto('/web/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#summary:not([hidden])');
    await page.waitForSelector('#session-panel:not([hidden])');
    assert.equal(await page.locator('#status').textContent(), 'Synthetic offline bundle loaded');
    assert.equal(await page.locator('#boundary').textContent(), 'adapter.cli_input');
    assert.equal(await page.locator('#component-rows tr').count(), 3);
    assert.equal(await page.locator('#timeline-list li').count(), 7);
    assert.equal(await page.locator('#session-mode').textContent(), 'fixture_only');
    assert.equal(await page.locator('#session-rows tr').count(), 0);
    assert.match(await page.locator('#session-methods').textContent(), /thread\/compact\/start/);
    await page.locator('#session-create-candidate').click();
    await page.waitForFunction(() => document.querySelectorAll('#session-rows tr').length === 1);
    assert.match(await page.locator('#session-message').textContent(), /accepted locally/);
    assert.equal(await page.locator('#session-operation-rows tr').count(), 1);
    assert.equal(
      await page.locator('#session-operation-rows tr').first().getAttribute('data-operation-state'),
      'intent_persisted',
    );
    const deniedSessionWrite = await (await fetch(`${base}/v1/runs/fixture-run-001/provider-sessions/candidates`, {
      method: 'POST',
      headers: {
        Authorization: 'Bearer fixture-session-token',
        'Content-Type': 'application/json',
        Origin: base,
      },
      body: JSON.stringify({
        idempotency_key: 'browser-denied-candidate',
        expected_control_generation: 0,
        approved_policy_ref: 'policy-fixture',
        profile_ref: 'profile-fixture',
        purpose: 'evaluation',
      }),
    })).status;
    assert.equal(deniedSessionWrite, 403);
    await page.locator('#compare-button').focus();
    assert.equal(await page.evaluate(() => document.activeElement.id), 'compare-button');
    await page.keyboard.press('Enter');
    assert.match(await page.locator('#comparison-result').textContent(), /retained snapshots differ|same ordered component measurements/);
    const reducedMotionMatch = await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches);
    assert.equal(reducedMotionMatch, true);
    const storage = await page.evaluate(async () => ({
      localStorageEntries: localStorage.length,
      sessionStorageEntries: sessionStorage.length,
      indexedDbDatabases: typeof indexedDB.databases === 'function' ? (await indexedDB.databases()).length : 0,
      cacheNames: await caches.keys(),
    }));
    assert.equal(storage.localStorageEntries, 0);
    assert.equal(storage.sessionStorageEntries, 0);
    assert.equal(storage.indexedDbDatabases, 0);
    assert.deepEqual(storage.cacheNames, []);
    assert.deepEqual(consoleErrors, []);
    assert.deepEqual(pageErrors, []);
    assert.deepEqual(requests.filter((url) => !url.startsWith(base)), []);
    assert.equal(requests.some((url) => url === `${base}/demo/snapshot`), true);
    assert.equal(requests.some((url) => url === `${base}/demo/comparison`), true);
    assert.equal(requests.some((url) => url === `${base}/demo/events`), true);
    assert.equal(requests.some((url) => url.includes('/fixtures/')), false);

    const adversarialPage = await context.newPage();
    const adversarialRequests = [];
    const adversarialConsoleErrors = [];
    const adversarialPageErrors = [];
    const payload = '<img src=x onerror=window.__payloadExecuted=true>';
    adversarialPage.on('request', (request) => adversarialRequests.push(request.url()));
    adversarialPage.on('console', (message) => { if (message.type() === 'error') adversarialConsoleErrors.push(message.text()); });
    adversarialPage.on('pageerror', (error) => adversarialPageErrors.push(String(error)));
    await adversarialPage.route(`${base}/demo/snapshot`, async (route) => {
      const response = await route.fetch();
      const snapshot = await response.json();
      snapshot.provider.model = payload;
      snapshot.components[0].kind = payload;
      await route.fulfill({ response, body: JSON.stringify(snapshot) });
    });
    await adversarialPage.goto('/web/', { waitUntil: 'networkidle' });
    await adversarialPage.waitForSelector('#summary:not([hidden])');
    assert.equal(await adversarialPage.locator('#provider-model').textContent(), payload);
    assert.equal(await adversarialPage.locator('#component-rows tr').first().locator('td').nth(1).textContent(), payload);
    assert.equal(await adversarialPage.evaluate(() => window.__payloadExecuted === true), false);
    assert.equal(await adversarialPage.locator('img, svg').count(), 0);
    assert.equal(await adversarialPage.locator('script:not([src])').count(), 0);
    assert.deepEqual(adversarialConsoleErrors, []);
    assert.deepEqual(adversarialPageErrors, []);
    assert.deepEqual(adversarialRequests.filter((url) => !url.startsWith(base)), []);
    await adversarialPage.close();

    const manifestPage = await context.newPage();
    const manifestRequests = [];
    const manifestConsoleErrors = [];
    const manifestPageErrors = [];
    manifestPage.on('request', (request) => manifestRequests.push(request.url()));
    manifestPage.on('console', (message) => { if (message.type() === 'error') manifestConsoleErrors.push(message.text()); });
    manifestPage.on('pageerror', (error) => manifestPageErrors.push(String(error)));
    await manifestPage.route(`${base}/offline-bundle.json`, async (route) => {
      const response = await route.fetch();
      const manifest = await response.json();
      manifest.snapshot = '\t/secret';
      await route.fulfill({ response, body: JSON.stringify(manifest) });
    });
    await manifestPage.goto('/web/', { waitUntil: 'networkidle' });
    await manifestPage.waitForSelector('#error:not([hidden])');
    assert.equal(await manifestPage.locator('#status').textContent(), 'Offline bundle unavailable');
    assert.match(await manifestPage.locator('#error-message').textContent(), /path is invalid/);
    assert.equal(manifestRequests.includes(`${base}/secret`), false);
    assert.deepEqual(manifestRequests.filter((url) => !url.startsWith(base)), []);
    assert.deepEqual(manifestConsoleErrors, []);
    assert.deepEqual(manifestPageErrors, []);
    await manifestPage.close();

    const statePage = await context.newPage();
    const stateRequests = [];
    const stateConsoleErrors = [];
    const statePageErrors = [];
    statePage.on('request', (request) => stateRequests.push(request.url()));
    statePage.on('console', (message) => { if (message.type() === 'error') stateConsoleErrors.push(message.text()); });
    statePage.on('pageerror', (error) => statePageErrors.push(String(error)));
    await statePage.route(`${base}/v1/runs/fixture-run-001/provider-sessions`, async (route) => {
      const response = await route.fetch();
      const list = await response.json();
      list.value.operations = [
        { operation_id: 'operation-accepted', kind: 'create_candidate', state: 'intent_persisted', automatic_retry: false, auto_resume: false, game_effects: 0 },
        { operation_id: 'operation-pending', kind: 'reconnect', state: 'sent', automatic_retry: false, auto_resume: false, game_effects: 0 },
        { operation_id: 'operation-unknown', kind: 'turn', state: 'unknown', automatic_retry: false, auto_resume: false, game_effects: 0 },
        { operation_id: 'operation-completed', kind: 'compact', state: 'completed', automatic_retry: false, auto_resume: false, game_effects: 0 },
      ];
      await route.fulfill({ response, body: JSON.stringify(list) });
    });
    await statePage.goto('/web/', { waitUntil: 'networkidle' });
    await statePage.waitForSelector('#session-panel:not([hidden])');
    await statePage.waitForFunction(() => document.querySelectorAll('#session-operation-rows tr').length === 4);
    const renderedStates = await statePage
      .locator('#session-operation-rows tr')
      .evaluateAll((rows) => rows.map((row) => row.dataset.operationState));
    assert.deepEqual(renderedStates, ['intent_persisted', 'sent', 'unknown', 'completed']);
    assert.equal(new Set(renderedStates).size, 4);
    assert.deepEqual(stateConsoleErrors, []);
    assert.deepEqual(statePageErrors, []);
    assert.deepEqual(stateRequests.filter((url) => !url.startsWith(base)), []);
    await statePage.close();

    const desktopPath = path.join(evidenceDir, 'integrated-browser-desktop.png');
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: desktopPath, fullPage: true });
    await page.setViewportSize({ width: 375, height: 800 });
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(100);
    const narrowPath = path.join(evidenceDir, 'integrated-browser-narrow.png');
    await page.screenshot({ path: narrowPath, fullPage: true });
    const horizontalOverflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
    assert.equal(horizontalOverflow, false);
    const metricsResponse = await fetch(`${base}/demo/metrics`);
    assert.equal(metricsResponse.ok, true);
    const metrics = await metricsResponse.json();
    const memoryHeaders = { Authorization: 'Bearer fixture-editor-token' };
    const beforeMemorySnapshot = Buffer.from(
      await (await fetch(`${base}/demo/snapshot`, { headers: memoryHeaders })).arrayBuffer(),
    );
    const memoryCapabilitiesResponse = await fetch(`${base}/v3/memory/capabilities`, {
      headers: memoryHeaders,
    });
    assert.equal(memoryCapabilitiesResponse.ok, true);
    const memoryCapabilities = await memoryCapabilitiesResponse.json();
    const memoryStatusResponse = await fetch(`${base}/v3/memory/status`, {
      headers: memoryHeaders,
    });
    assert.equal(memoryStatusResponse.ok, true);
    const memoryStatus = await memoryStatusResponse.json();
    const afterMemorySnapshot = Buffer.from(
      await (await fetch(`${base}/demo/snapshot`, { headers: memoryHeaders })).arrayBuffer(),
    );
    assert.equal(memoryCapabilities.enabled, false);
    assert.deepEqual(memoryCapabilities.supported_operations, []);
    assert.equal(memoryStatus.inference_calls, 0);
    assert.equal(memoryStatus.effect_class, 'local_read_no_inference');
    assert.deepEqual(afterMemorySnapshot, beforeMemorySnapshot);
    assert.equal(metrics.producer_snapshots, 2);
    assert.equal(metrics.capture_records, 2);
    assert.equal(metrics.producer_events, 7);
    assert.equal(metrics.provider_calls, 0);
    assert.equal(metrics.game_launches, 0);
    assert.equal(metrics.external_requests, 0);
    assert.equal(metrics.read_only, true);
    assert.ok(metrics.api_requests >= 3);
    assert.ok(metrics.browser_requests >= 10);
    const manifestResponse = await fetch(`${base}/offline-bundle.json`);
    assert.equal(manifestResponse.ok, true);
    const manifestBytes = Buffer.from(await manifestResponse.arrayBuffer());
    const manifestSha256 = crypto.createHash('sha256').update(manifestBytes).digest('hex');

    const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }).trim();
    const untracked = git('ls-files', '--others', '--exclude-standard', '-z').split('\0').filter(Boolean);
    const completedAt = new Date().toISOString();
    const evidence = {
      schema: 'ascension.integrated-browser-evidence.v1',
      evidence_id: `INTEGRATED-BROWSER-${completedAt}`,
      completed_at: completedAt,
      requirement_ids: ['P1-025', 'P1-031', 'P1-032', 'P1-033', 'R04-001', 'R04-010', 'R04-060'],
      case_ids: ['INTEGRATED-NORMAL-001', 'INTEGRATED-ADVERSARIAL-001', 'INTEGRATED-MANIFEST-PATH-001', 'PHASE4-SESSION-001'],
      repository: {
        name: 'AI-Ascension/ascension-context-console',
        branch: require('node:child_process').execFileSync('git', ['branch', '--show-current'], { cwd: root, encoding: 'utf8' }).trim(),
        revision: require('node:child_process').execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
        working_tree_dirty: Boolean(git('status', '--porcelain')),
        tracked_diff_sha256: crypto.createHash('sha256').update(execFileSync('git', ['diff', '--binary', 'HEAD'], { cwd: root, maxBuffer: 32 * 1024 * 1024 })).digest('hex'),
        untracked_inputs: untracked.map((name) => ({ path: name, sha256: sha256(path.join(root, name)) })),
      },
      platform: {
        os: require('node:os').platform(),
        release: require('node:os').release(),
        architecture: require('node:os').arch(),
      },
      command: {
        server: prebuiltBinary ? 'prebuilt context-console integrated-demo 0' : 'cargo run --locked --package context-service --bin context-console -- integrated-demo 0',
        prebuilt_binary_sha256: prebuiltBinary ? sha256(prebuiltBinary) : null,
        audit: auditCommand,
      },
      exit_code: 0,
      result: 'passed',
      evidence_class: ['synthetic', 'compiled_peer'],
      tool: `Playwright ${playwrightVersion}`,
      browser: await browser.version(),
      base_url: base,
      viewport_desktop: { width: 1440, height: 1000 },
      viewport_narrow: { width: 375, height: 800 },
      reduced_motion: true,
      reduced_motion_match: reducedMotionMatch,
      requests,
      external_requests: [],
      fixture_requests: requests.filter((url) => url.includes('/fixtures/')),
      storage,
      horizontal_overflow_narrow: horizontalOverflow,
      keyboard_focus_id: 'compare-button',
      console_errors: consoleErrors,
      page_errors: pageErrors,
      adversarial_payload: {
        payload,
        text_content_preserved: true,
        markup_nodes_created: false,
        script_executed: false,
        external_requests: [],
        console_errors: adversarialConsoleErrors,
        page_errors: adversarialPageErrors,
      },
      manifest_path_rejection: {
        forbidden_path_requests: manifestRequests.filter((url) => url.endsWith('/secret')),
        external_requests: [],
        console_errors: manifestConsoleErrors,
        page_errors: manifestPageErrors,
        rejected_before_fetch: true,
      },
      memory_disabled_shadow: {
        enabled: memoryCapabilities.enabled,
        supported_operations: memoryCapabilities.supported_operations,
        inference_calls: memoryStatus.inference_calls,
        effect_class: memoryStatus.effect_class,
        snapshot_bytes_unchanged: afterMemorySnapshot.equals(beforeMemorySnapshot),
      },
      phase4_session: {
        mode: 'fixture_only',
        capability_profile: await page.locator('#session-profile').textContent(),
        candidate_created: true,
        binding_rows: await page.locator('#session-rows tr').count(),
        denied_without_csrf_status: deniedSessionWrite,
        native_calls: 0,
        game_effects: 0,
        rendered_operation_states: renderedStates,
      },
      integration_metrics: metrics,
      source_artifacts: [
        { path: 'crates/context-service/src/demo/mod.rs', sha256: sha256(path.join(root, 'crates/context-service/src/demo/mod.rs')) },
        { path: 'crates/context-service/src/demo/fixtures.rs', sha256: sha256(path.join(root, 'crates/context-service/src/demo/fixtures.rs')) },
        { path: 'crates/context-service/src/demo/routes.rs', sha256: sha256(path.join(root, 'crates/context-service/src/demo/routes.rs')) },
        { path: 'crates/context-service/src/demo/server.rs', sha256: sha256(path.join(root, 'crates/context-service/src/demo/server.rs')) },
        { path: 'crates/context-service/src/demo/state.rs', sha256: sha256(path.join(root, 'crates/context-service/src/demo/state.rs')) },
        { path: 'tools/browser-audit/integrated_browser_audit.cjs', sha256: sha256(__filename) },
        { path: 'web/index.html', sha256: sha256(path.join(root, 'web', 'index.html')) },
        { path: 'web/css/styles.css', sha256: sha256(path.join(root, 'web', 'css', 'styles.css')) },
        { path: 'web/js/app.js', sha256: sha256(path.join(root, 'web', 'js', 'app.js')) },
        { path: 'web/js/api.js', sha256: sha256(path.join(root, 'web', 'js', 'api.js')) },
        { path: 'web/js/bundle.js', sha256: sha256(path.join(root, 'web', 'js', 'bundle.js')) },
        { path: 'web/js/render.js', sha256: sha256(path.join(root, 'web', 'js', 'render.js')) },
        { path: 'fixtures/valid/snapshot-metadata.json', sha256: sha256(path.join(root, 'fixtures', 'valid', 'snapshot-metadata.json')) },
        { path: 'fixtures/valid/snapshot-cli.json', sha256: sha256(path.join(root, 'fixtures', 'valid', 'snapshot-cli.json')) },
        { path: 'fixtures/valid/events.jsonl', sha256: sha256(path.join(root, 'fixtures', 'valid', 'events.jsonl')) },
        { path: 'offline-bundle.json', sha256: sha256(path.join(root, 'offline-bundle.json')) },
        { path: 'served:/offline-bundle.json', sha256: manifestSha256 },
      ],
      assertions: {
        producer_stage: metrics.producer_snapshots === 2,
        capture_stage: metrics.capture_records === 2,
        api_stage: metrics.api_requests >= 3,
        browser_stage: metrics.browser_requests >= 10,
        zero_provider_calls: metrics.provider_calls === 0,
        zero_game_launches: metrics.game_launches === 0,
        zero_external_requests: metrics.external_requests === 0,
        normal_flow: true,
        adversarial_payload_text_only: true,
        manifest_path_rejected: true,
        keyboard_comparison: true,
        reduced_motion: reducedMotionMatch,
        no_browser_persistence: true,
        no_horizontal_overflow_narrow: true,
        memory_disabled_shadow: memoryCapabilities.enabled === false
          && memoryCapabilities.supported_operations.length === 0
          && memoryStatus.inference_calls === 0
          && afterMemorySnapshot.equals(beforeMemorySnapshot),
        phase4_session_fixture: true,
        phase4_csrf_denied: deniedSessionWrite === 403,
        phase4_async_states_distinct: new Set(renderedStates).size === 4,
      },
      artifacts: [
        { path: desktopPath, sha256: sha256(desktopPath) },
        { path: narrowPath, sha256: sha256(narrowPath) },
      ],
    };
    const evidencePath = path.join(evidenceDir, 'integrated-browser-ui.json');
    fs.writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
    console.log(JSON.stringify(evidence, null, 2));
  } finally {
    try {
      if (browser) await browser.close();
    } finally {
      await stopServer(child);
    }
    if (serverStderr && process.env.SHOW_SERVER_STDERR === '1') process.stderr.write(serverStderr);
  }
}

run().catch((error) => {
  console.error(error.stack || error);
  process.exitCode = 1;
});
