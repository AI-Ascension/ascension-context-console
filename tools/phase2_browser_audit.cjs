// SPDX-License-Identifier: MIT

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const evidenceDir = path.join(root, 'docs', 'evidence');
const playwrightModule = process.env.PLAYWRIGHT_MODULE || 'playwright';
const { chromium } = require(playwrightModule);

function digest(file) {
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
    child.once('exit', (code, signal) => reject(new Error(`server exited before readiness: ${code}/${signal}`)));
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
    { cwd: root, env: process.env, stdio: ['ignore', 'pipe', 'pipe'] },
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
    await page.waitForSelector('#control-panel:not([hidden])');
    const offlineManifest = await page.evaluate(async () => (await fetch('/offline-bundle.json', { cache: 'no-store' })).json());
    assert.equal(Object.values(offlineManifest).some((value) => typeof value === 'string' && (value.includes('/v2/') || value.includes('run-fixture-001'))), false);
    assert.equal(await page.locator('#management-badge').textContent(), 'management enabled');
    assert.equal(await page.locator('#control-status').textContent(), 'running');
    assert.equal(await page.locator('#durable-store').textContent(), 'supported');
    assert.ok(await page.locator('#eligible-rows input[type=checkbox]').count() >= 1);

    await page.locator('#create-draft').focus();
    assert.equal(await page.evaluate(() => document.activeElement?.id), 'create-draft');
    await page.keyboard.press('Enter');
    await page.waitForFunction(() => document.querySelector('#draft-version').textContent.includes('version 1'));
    await page.locator('#eligible-rows input[type=checkbox]').first().check();
    await page.locator('#eligible-rows input.context-pin').first().check();
    await page.locator('#note-text').fill('Browser operator note <img src="https://attacker.invalid/note.png" onerror="window.__noteInjection=1"> for the controlled fixture.');
    await page.locator('#objective-text').fill('Preserve the fixture while choosing visible legal actions.');
    await page.locator('#save-draft').click();
    await page.waitForFunction(() => document.querySelector('#draft-message').textContent.includes('Draft saved'));
    assert.match(await page.locator('#draft-version').textContent(), /version 2/);
    assert.equal(await page.locator('#eligible-rows input.context-pin:checked').count(), 1);

    await page.locator('#preview-draft').click();
    await page.waitForFunction(() => document.querySelector('#preview-badge').textContent.includes('exploratory'));
    assert.equal(await page.locator('#commit-draft').isDisabled(), true);

    await page.locator('#pause-run').click();
    await page.waitForFunction(() => document.querySelector('#control-status').textContent === 'paused_ready');
    const reconnect = await context.newPage();
    try {
      await reconnect.goto('/web/', { waitUntil: 'networkidle' });
      await reconnect.waitForSelector('#control-panel:not([hidden])');
      assert.equal(await reconnect.locator('#control-status').textContent(), 'paused_ready');
      assert.equal(await reconnect.locator('#management-badge').textContent(), 'management enabled');
    } finally {
      await reconnect.close();
    }
    await page.locator('#budget-risk-ack').check();
    await page.locator('#preview-draft').click();
    await page.waitForFunction(() => document.querySelector('#preview-badge').textContent === 'applicable');
    assert.equal(await page.locator('#commit-draft').isDisabled(), false);
    assert.notEqual(await page.locator('#prepared-digest').textContent(), 'unavailable');
    assert.match(await page.locator('#preview-budget').textContent(), /bounded_unknown_total/);
    assert.match(await page.locator('#preview-diff').textContent(), /note-browser/);
    assert.match(await page.locator('#note-text').inputValue(), /<img src=/);
    assert.equal(await page.evaluate(() => window.__noteInjection), undefined);

    await page.locator('#commit-draft').click();
    await page.waitForFunction(() => document.querySelector('#control-status').textContent === 'paused_committed');
    assert.match(await page.locator('#draft-message').textContent(), /committed while paused/);
    assert.equal(await page.locator('#preview').isHidden(), true);
    await page.locator('#resume-run').click();
    await page.waitForFunction(() => document.querySelector('#control-status').textContent === 'running');
    assert.match(await page.locator('#draft-message').textContent(), /Resume/);

    await page.locator('#restore-source').selectOption('revision-1');
    await page.locator('#restore-draft').click();
    await page.waitForFunction(() => document.querySelector('#draft-message').textContent.includes('restored'));
    assert.equal(await page.locator('#eligible-rows input.context-select:checked').count(), 0);
    assert.equal(await page.locator('#preview').isHidden(), true);
    const conflictProbe = await page.evaluate(async () => {
      const draft = await (await fetch('/v2/runs/fixture-run/context-control/drafts/draft-1', { headers: { Authorization: 'Bearer fixture-editor-token' } })).json();
      const state = await (await fetch('/v2/runs/fixture-run/context-control/state', { headers: { Authorization: 'Bearer fixture-editor-token' } })).json();
      const eligible = await (await fetch('/v2/runs/fixture-run/context-control/eligible-items', { headers: { Authorization: 'Bearer fixture-editor-token' } })).json();
      const item = eligible.items.find((entry) => !entry.protected).item;
      const response = await fetch(`/v2/runs/fixture-run/context-control/drafts/${draft.draft_id}/operations`, {
        method: 'POST',
        headers: { Authorization: 'Bearer fixture-editor-token', Origin: location.origin, 'X-CSRF-Token': 'fixture-csrf-token', 'Content-Type': 'application/json' },
        body: JSON.stringify({
          schema: 'ascension.context-control.patch.v1',
          scope: draft.scope,
          draft_id: draft.draft_id,
          expected_draft_version: draft.version,
          expected_active_revision_id: state.active_revision_id,
          operations: [{ op: 'include_item', item }],
        }),
      });
      return { status: response.status, value: await response.json() };
    });
    assert.equal(conflictProbe.status, 200);
    await page.locator('#note-text').fill('stale browser edit');
    await page.locator('#save-draft').click();
    await page.waitForFunction(() => document.querySelector('#draft-message').dataset.state === 'error');
    assert.match(await page.locator('#draft-message').textContent(), /draft version is stale/);
    const workflowConsoleErrors = consoleErrors.slice();
    const workflowPageErrors = pageErrors.slice();
    const expectedConflictConsoleErrors = workflowConsoleErrors.filter((message) => message.includes('status of 409'));
    assert.equal(expectedConflictConsoleErrors.length, 1);
    assert.deepEqual(workflowConsoleErrors.filter((message) => !message.includes('status of 409')), []);
    assert.deepEqual(workflowPageErrors, []);

    const security = await page.evaluate(async () => {
      const scope = { project_id: 'fixture-project', run_id: 'fixture-run', episode_id: 'fixture-episode', agent_id: 'fixture-agent' };
      const body = JSON.stringify({ scope, expected_active_revision_id: 'revision-1' });
      const readOnly = await fetch('/v2/runs/fixture-run/context-control/drafts', {
        method: 'POST',
        headers: { Authorization: 'Bearer integrated-demo-token', Origin: location.origin, 'X-CSRF-Token': 'fixture-csrf-token', 'Content-Type': 'application/json' },
        body,
      });
      const missingCsrf = await fetch('/v2/runs/fixture-run/context-control/drafts', {
        method: 'POST',
        headers: { Authorization: 'Bearer fixture-editor-token', 'Content-Type': 'application/json' },
        body,
      });
      const duplicate = await fetch('/v2/runs/fixture-run/context-control/drafts', {
        method: 'POST',
        headers: { Authorization: 'Bearer fixture-editor-token', Origin: location.origin, 'X-CSRF-Token': 'fixture-csrf-token', 'Content-Type': 'application/json' },
        body: `{"scope":${JSON.stringify(scope)},"scope":${JSON.stringify(scope)},"expected_active_revision_id":"revision-1"}`,
      });
      const otherRun = await fetch('/v2/runs/other-run/context-control/state', {
        headers: { Authorization: 'Bearer fixture-editor-token' },
      });
      const draftBeforeForgedIdentity = await (await fetch('/v2/runs/fixture-run/context-control/drafts/draft-1', {
        headers: { Authorization: 'Bearer fixture-editor-token' },
      })).json();
      const eligible = await (await fetch('/v2/runs/fixture-run/context-control/eligible-items', {
        headers: { Authorization: 'Bearer fixture-editor-token' },
      })).json();
      const item = eligible.items.find((entry) => !entry.protected).item;
      const forgedIdentity = await fetch('/v2/runs/fixture-run/context-control/drafts/draft-1/operations', {
        method: 'POST',
        headers: { Authorization: 'Bearer fixture-editor-token', Origin: location.origin, 'X-CSRF-Token': 'fixture-csrf-token', 'Content-Type': 'application/json' },
        body: JSON.stringify({
          schema: 'ascension.context-control.patch.v1',
          scope,
          draft_id: 'draft-1',
          expected_draft_version: draftBeforeForgedIdentity.version,
          expected_active_revision_id: 'revision-1',
          operations: [{ op: 'include_item', item }],
          actor: 'forged-operator',
          role: 'admin',
        }),
      });
      const draftAfterForgedIdentity = await (await fetch('/v2/runs/fixture-run/context-control/drafts/draft-1', {
        headers: { Authorization: 'Bearer fixture-editor-token' },
      })).json();
      return {
        readOnly: { status: readOnly.status, value: await readOnly.json() },
        missingCsrf: { status: missingCsrf.status, value: await missingCsrf.json() },
        duplicate: { status: duplicate.status, value: await duplicate.json() },
        otherRun: { status: otherRun.status, value: await otherRun.json() },
        forgedIdentity: {
          status: forgedIdentity.status,
          value: await forgedIdentity.json(),
          versionBefore: draftBeforeForgedIdentity.version,
          versionAfter: draftAfterForgedIdentity.version,
        },
      };
    });
    const crossOriginResponse = await fetch(`${base}/v2/runs/fixture-run/context-control/drafts`, {
      method: 'POST',
      headers: {
        Authorization: 'Bearer fixture-editor-token',
        Origin: 'https://attacker.invalid',
        'X-CSRF-Token': 'fixture-csrf-token',
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        scope: { project_id: 'fixture-project', run_id: 'fixture-run', episode_id: 'fixture-episode', agent_id: 'fixture-agent' },
        expected_active_revision_id: 'revision-1',
      }),
    });
    const crossOrigin = { status: crossOriginResponse.status, value: await crossOriginResponse.json() };
    assert.equal(security.readOnly.status, 403);
    assert.equal(security.missingCsrf.status, 403);
    assert.equal(crossOrigin.status, 403);
    assert.equal(security.duplicate.status, 422);
    assert.equal(security.duplicate.value.error.code, 'duplicate_json_key');
    assert.notEqual(security.otherRun.status, 200);
    assert.equal(security.otherRun.value.error.code, 'route_not_found');
    assert.equal(security.forgedIdentity.status, 422);
    assert.equal(security.forgedIdentity.value.error.code, 'invalid_json');
    assert.equal(security.forgedIdentity.versionAfter, security.forgedIdentity.versionBefore);

    const events = await page.evaluate(async () => (await fetch('/v2/runs/fixture-run/context-control/events', { cache: 'no-store', headers: { Authorization: 'Bearer fixture-editor-token' } })).json());
    const eventTypes = events.events.map((event) => event.event_type);
    for (const expected of ['draft.updated', 'preview.built', 'pause.accepted', 'pause.ready', 'revision.committed', 'plan.retired', 'resume.accepted']) {
      assert.ok(eventTypes.includes(expected), `missing control event ${expected}`);
    }
    const revisions = await page.evaluate(async () => (await fetch('/v2/runs/fixture-run/context-control/revisions', { cache: 'no-store', headers: { Authorization: 'Bearer fixture-editor-token' } })).json());
    assert.ok(revisions.revisions.length >= 2);
    const metrics = await page.evaluate(async () => (await fetch('/demo/metrics', { cache: 'no-store' })).json());
    assert.equal(metrics.provider_calls, 0);
    assert.equal(metrics.game_launches, 0);
    assert.equal(metrics.external_requests, 0);
    assert.equal(metrics.management_enabled, true);
    assert.equal(metrics.durable_control_store, 'supported');
    assert.ok(metrics.control_events >= 7);
    assert.equal(JSON.stringify(metrics).includes('Browser operator note'), false);
    assert.equal(JSON.stringify(metrics).includes('<img'), false);

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
    assert.deepEqual(pageErrors, []);
    assert.deepEqual(requests.filter((url) => !url.startsWith(base)), []);

    await page.evaluate(() => document.fonts.ready);
    const desktopPath = path.join(evidenceDir, 'phase2-browser-desktop-20260910.png');
    const narrowPath = path.join(evidenceDir, 'phase2-browser-narrow-20260910.png');
    fs.mkdirSync(evidenceDir, { recursive: true });
    await page.screenshot({ path: desktopPath, fullPage: true });
    await page.setViewportSize({ width: 375, height: 800 });
    await page.waitForTimeout(100);
    await page.screenshot({ path: narrowPath, fullPage: true });
    const horizontalOverflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
    assert.equal(horizontalOverflow, false);

    const evidence = {
      schema: 'ascension.phase2-browser-evidence.v1',
      evidence_id: 'PHASE2-BROWSER-20260910',
      requirement_ids: ['P2-R010', 'P2-R013', 'P2-R014', 'P2-R015', 'P2-R017', 'P2-R018', 'P2-R036', 'P2-R038', 'P2-R044', 'P2-R045', 'P2-R046', 'P2-R049', 'P2-R050', 'P2-R051', 'P2-R052', 'P2-R053', 'P2-R059', 'P2-R063', 'P2-R064'],
      case_ids: ['P2-F001', 'P2-F003', 'P2-F004', 'P2-F005', 'P2-F006', 'P2-F007', 'P2-F010', 'P2-F011', 'P2-F012', 'P2-F017', 'P2-F022', 'P2-F031', 'P2-F073', 'P2-F074', 'P2-F075', 'P2-F076'],
      repository: {
        name: 'AI-Ascension/ascension-context-console',
        branch: require('node:child_process').execFileSync('git', ['branch', '--show-current'], { cwd: root, encoding: 'utf8' }).trim(),
        revision: require('node:child_process').execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
      },
      command: {
        server: 'cargo run --locked --package context-service --bin context-console -- integrated-demo 0',
        audit: `FONTCONFIG_PATH=${process.env.FONTCONFIG_PATH || '<unset>'} FONTCONFIG_FILE=${process.env.FONTCONFIG_FILE || '<unset>'} XDG_DATA_DIRS=${process.env.XDG_DATA_DIRS || '<unset>'} LD_LIBRARY_PATH=${process.env.LD_LIBRARY_PATH || '<unset>'} PLAYWRIGHT_MODULE=${playwrightModule} node tools/phase2_browser_audit.cjs`,
      },
      result: 'passed',
      evidence_class: ['synthetic', 'native', 'browser'],
      tool: `Playwright ${require(`${playwrightModule}/package.json`).version}`,
      browser: await browser.version(),
      base_url: base,
      workflow: ['draft_created', 'draft_saved', 'exploratory_preview', 'pause_ready', 'applicable_preview', 'commit_paused', 'explicit_resume', 'restore_as_draft'],
      event_types: eventTypes,
      security,
      requests,
      storage,
      horizontal_overflow_narrow: horizontalOverflow,
      console_errors: workflowConsoleErrors,
      security_console_errors: consoleErrors.slice(workflowConsoleErrors.length),
      page_errors: pageErrors,
      integration_metrics: metrics,
      assertions: {
        exact_control_workflow: true,
        objective_and_note_delivered: true,
        pin_and_restore_are_typed_draft_operations: true,
        applicable_preview_has_digest: true,
        unknown_total_capacity_acknowledged: true,
        commit_remains_paused: true,
        resume_is_explicit: true,
        zero_provider_calls: metrics.provider_calls === 0,
        zero_game_launches: metrics.game_launches === 0,
        zero_external_requests: metrics.external_requests === 0,
        no_browser_persistence: true,
        no_horizontal_overflow_narrow: true,
        read_only_token_denied: security.readOnly.status === 403,
        cross_origin_write_denied: crossOrigin.status === 403,
        missing_csrf_denied: security.missingCsrf.status === 403,
        duplicate_json_rejected: security.duplicate.value.error.code === 'duplicate_json_key',
        cross_scope_route_concealed: security.otherRun.value.error.code === 'route_not_found',
        forged_actor_role_rejected_without_mutation: security.forgedIdentity.value.error.code === 'invalid_json' && security.forgedIdentity.versionAfter === security.forgedIdentity.versionBefore,
        note_html_stays_data: true,
        private_note_absent_from_metrics: !JSON.stringify(metrics).includes('Browser operator note') && !JSON.stringify(metrics).includes('<img'),
        reconnect_preserves_pause_without_auto_resume: true,
        keyboard_activation: true,
        stale_preview_is_cleared_after_mutation: true,
        offline_manifest_is_read_only: true,
        conflict_error_is_shown: true,
      },
      artifacts: [
        { path: 'docs/evidence/phase2-browser-desktop-20260910.png', sha256: digest(desktopPath) },
        { path: 'docs/evidence/phase2-browser-narrow-20260910.png', sha256: digest(narrowPath) },
      ],
    };
    fs.writeFileSync(path.join(evidenceDir, 'phase2-browser-ui-20260910.json'), `${JSON.stringify(evidence, null, 2)}\n`);
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
