// SPDX-License-Identifier: MIT

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');

const root = path.resolve(__dirname, '..', '..');
const playwrightModule = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium } = require(playwrightModule);
const evidenceDir = path.join(root, 'docs', 'evidence');
fs.mkdirSync(evidenceDir, { recursive: true });
const auditCommand = [
  `FONTCONFIG_PATH=${process.env.FONTCONFIG_PATH || '<unset>'}`,
  `FONTCONFIG_FILE=${process.env.FONTCONFIG_FILE || '<unset>'}`,
  `XDG_DATA_DIRS=${process.env.XDG_DATA_DIRS || '<unset>'}`,
  `LD_LIBRARY_PATH=${process.env.LD_LIBRARY_PATH || '<unset>'}`,
  `PLAYWRIGHT_MODULE=${playwrightModule}`,
  'node tools/browser-audit/integrated_browser_audit.cjs',
].join(' ');

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function waitForServer(child) {
  return new Promise((resolve, reject) => {
    let output = '';
    const onData = (chunk) => {
      output += chunk.toString();
      const line = output.split(/\r?\n/).find((value) => value.startsWith('integrated_demo_ready='));
      if (line) {
        child.stdout.off('data', onData);
        resolve(line.slice('integrated_demo_ready='.length).trim());
      }
    };
    child.stdout.on('data', onData);
    child.once('error', reject);
    child.once('exit', (code, signal) => reject(new Error(`integrated demo exited before readiness: ${code}/${signal}`)));
  });
}

async function stopServer(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill('SIGTERM');
  await new Promise((resolve) => child.once('exit', resolve));
}

async function run() {
  const child = spawn(
    process.env.CONTEXT_CONSOLE_BIN || 'cargo',
    process.env.CONTEXT_CONSOLE_BIN
      ? []
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
    const base = new URL(readyUrl).origin;
    browser = await chromium.launch({ headless: true });
    const context = await browser.newContext({
      baseURL: base,
      reducedMotion: 'reduce',
      viewport: { width: 1440, height: 1000 },
    });
    const page = await context.newPage();
    const requests = [];
    const consoleErrors = [];
    const pageErrors = [];
    page.on('request', (request) => requests.push(request.url()));
    page.on('console', (message) => { if (message.type() === 'error') consoleErrors.push(message.text()); });
    page.on('pageerror', (error) => pageErrors.push(String(error)));
    await page.goto('/web/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#summary:not([hidden])');
    assert.equal(await page.locator('#status').textContent(), 'Synthetic offline bundle loaded');
    assert.equal(await page.locator('#boundary').textContent(), 'adapter.cli_input');
    assert.equal(await page.locator('#component-rows tr').count(), 3);
    assert.equal(await page.locator('#timeline-list li').count(), 7);
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

    const desktopPath = path.join(evidenceDir, 'integrated-browser-desktop-20260910.png');
    await page.evaluate(() => document.fonts.ready);
    await page.screenshot({ path: desktopPath, fullPage: true });
    await page.setViewportSize({ width: 375, height: 800 });
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(100);
    const narrowPath = path.join(evidenceDir, 'integrated-browser-narrow-20260910.png');
    await page.screenshot({ path: narrowPath, fullPage: true });
    const horizontalOverflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
    assert.equal(horizontalOverflow, false);
    const metricsResponse = await fetch(`${base}/demo/metrics`);
    assert.equal(metricsResponse.ok, true);
    const metrics = await metricsResponse.json();
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

    const evidence = {
      schema: 'ascension.integrated-browser-evidence.v1',
      evidence_id: 'INTEGRATED-BROWSER-20260910',
      requirement_ids: ['P1-025', 'P1-031', 'P1-032', 'P1-033'],
      case_ids: ['INTEGRATED-NORMAL-001', 'INTEGRATED-ADVERSARIAL-001', 'INTEGRATED-MANIFEST-PATH-001'],
      repository: {
        name: 'AI-Ascension/ascension-context-console',
        branch: 'phase1/t02-bootstrap',
        revision: require('node:child_process').execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
      },
      platform: {
        os: require('node:os').platform(),
        release: require('node:os').release(),
        architecture: require('node:os').arch(),
      },
      command: {
        server: 'cargo run --locked --package context-service --bin context-console -- integrated-demo 0',
        audit: auditCommand,
      },
      exit_code: 0,
      result: 'passed',
      evidence_class: ['synthetic', 'native'],
      tool: `Playwright ${require(playwrightModule + '/package.json').version}`,
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
      },
      artifacts: [
        { path: 'docs/evidence/integrated-browser-desktop-20260910.png', sha256: sha256(desktopPath) },
        { path: 'docs/evidence/integrated-browser-narrow-20260910.png', sha256: sha256(narrowPath) },
      ],
    };
    const evidencePath = path.join(evidenceDir, 'integrated-browser-ui-20260910.json');
    fs.writeFileSync(evidencePath, `${JSON.stringify(evidence, null, 2)}\n`);
    console.log(JSON.stringify(evidence, null, 2));
  } finally {
    if (browser) await browser.close();
    await stopServer(child);
    if (serverStderr && process.env.SHOW_SERVER_STDERR === '1') process.stderr.write(serverStderr);
  }
}

run().catch((error) => {
  console.error(error.stack || error);
  process.exitCode = 1;
});
