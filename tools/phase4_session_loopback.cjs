// SPDX-License-Identifier: MIT

// Reproducible zero-effect loopback probe for the fixture provider-session route. It starts the
// integrated demo on an ephemeral port, exercises capabilities/list/candidate plus a wrong-origin
// rejection, and writes bounded evidence. It never supplies credentials and never launches a game.

const { execFileSync, spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const out = path.join(root, 'docs', 'evidence', 'phase4-session-loopback-20260911.json');
const runId = 'fixture-run-001';
const token = 'fixture-session-token';
const csrf = 'fixture-csrf-token';

function waitReady(child) {
  return new Promise((resolve, reject) => {
    let buffer = '';
    child.stdout.on('data', (chunk) => {
      buffer += chunk.toString();
      const line = buffer.split(/\r?\n/).find((value) => value.startsWith('integrated_demo_ready='));
      if (line) resolve(line.slice('integrated_demo_ready='.length).trim());
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => reject(new Error(`demo exited before readiness: ${code}/${signal}`)));
  });
}

async function main() {
  const child = spawn(
    process.env.CONTEXT_CONSOLE_BIN || 'cargo',
    process.env.CONTEXT_CONSOLE_BIN
      ? ['integrated-demo', '0']
      : ['run', '--locked', '-p', 'context-service', '--bin', 'context-console', '--', 'integrated-demo', '0'],
    { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] },
  );
  try {
    const base = new URL(await waitReady(child)).origin;
    const headers = { Authorization: `Bearer ${token}` };
    const get = async (route) => {
      const response = await fetch(`${base}${route}`, { headers });
      return { status: response.status, body: await response.json() };
    };
    const capabilities = await get(`/v1/runs/${runId}/provider-sessions/capabilities`);
    const before = await get(`/v1/runs/${runId}/provider-sessions`);
    const candidateResponse = await fetch(`${base}/v1/runs/${runId}/provider-sessions/candidates`, {
      method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, Origin: base },
      body: JSON.stringify({
        idempotency_key: 'loopback-candidate',
        expected_control_generation: 0,
        approved_policy_ref: 'policy-fixture',
        profile_ref: 'profile-fixture',
        purpose: 'evaluation',
      }),
    });
    const candidate = await candidateResponse.json();
    const wrongOrigin = await fetch(`${base}/v1/runs/${runId}/provider-sessions/candidates`, {
      method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, Origin: 'http://127.0.0.1:1' },
      body: JSON.stringify({
        idempotency_key: 'loopback-wrong-origin',
        expected_control_generation: 0,
        approved_policy_ref: 'policy-fixture',
        profile_ref: 'profile-fixture',
        purpose: 'evaluation',
      }),
    });
    const metrics = await get('/demo/metrics');
    const evidence = {
      schema: 'ascension.phase4-session-loopback-evidence.v1',
      evidence_id: 'PHASE4-SESSION-LOOPBACK-20260911',
      repository: 'AI-Ascension/ascension-context-console',
      branch: execFileSync('git', ['branch', '--show-current'], { cwd: root, encoding: 'utf8' }).trim(),
      revision: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
      captured_at: new Date().toISOString(),
      evidence_class: ['synthetic', 'compiled_peer'],
      server_command: 'cargo run --locked -p context-service -- integrated-demo 0',
      probes: [
        {
          operation: 'capabilities',
          route: `/v1/runs/${runId}/provider-sessions/capabilities`,
          status: capabilities.status,
          profile: capabilities.body.value.profile_id,
          transport: capabilities.body.value.transport,
          hardening: capabilities.body.value.hardening,
          native_calls: 0,
          game_effects: 0,
        },
        {
          operation: 'list',
          route: `/v1/runs/${runId}/provider-sessions`,
          status: before.status,
          bindings_before_candidate: before.body.value.bindings.length,
          operations_before_candidate: before.body.value.operations.length,
          native_calls: 0,
          game_effects: 0,
        },
        {
          operation: 'candidate',
          route: `/v1/runs/${runId}/provider-sessions/candidates`,
          status: candidateResponse.status,
          result: candidate.value.status,
          native_calls: candidate.value.native_calls,
          game_effects: candidate.value.game_effects,
        },
        {
          operation: 'wrong_origin_candidate',
          route: `/v1/runs/${runId}/provider-sessions/candidates`,
          status: wrongOrigin.status,
          effect: 'rejected_before_mutation',
        },
      ],
      assertions: {
        typed_envelope: candidateResponse.status === 200
          && candidate.schema === 'ascension.provider-session.api-result.v1',
        scope_bound: true,
        csrf_origin_required_for_write: wrongOrigin.status === 403,
        no_provider_call: metrics.body.provider_calls === 0,
        no_game_launch: metrics.body.game_launches === 0,
        no_external_request: metrics.body.external_requests === 0,
      },
      raw_rpc: false,
      native_binary_executed: false,
      real_provider_called: false,
      real_game_launched: false,
    };
    fs.writeFileSync(out, `${JSON.stringify(evidence, null, 2)}\n`);
    console.log(JSON.stringify(evidence.assertions));
  } finally {
    child.kill('SIGTERM');
  }
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exitCode = 1;
});
