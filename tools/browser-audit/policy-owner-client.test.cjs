// SPDX-License-Identifier: MIT

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

const moduleBytes = fs.readFileSync(path.join(__dirname, '../../web/js/policy-owner.js'));
const loadClient = () => import(`data:text/javascript;base64,${moduleBytes.toString('base64')}`);
const policyDigest = 'a'.repeat(64);
const targetDigest = 'b'.repeat(64);
const proposalDigest = 'c'.repeat(64);
const ownerToken = 'owner-token-only-in-header';
const runId = 'run.current';
const commandSchema = 'ascension.provider-session.policy-owner-command.v1';

function command(operation, revision, policySha256 = null, proposalSha256 = null) {
  return {
    schema_version: commandSchema,
    operation,
    revision,
    policy_sha256: policySha256,
    proposal_sha256: proposalSha256,
    effect_class: 'local_metadata_only',
    inference_calls: 0,
    game_effects: 0,
  };
}

function response(value) {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  });
}

test('saved-policy client calls authenticated same-origin routes with exact bytes and CAS revisions', async () => {
  const client = await loadClient();
  const sourceBytes = Buffer.from('{\n  "policy_id": "source"\n}\n');
  const targetBytes = Buffer.from('{\n  "policy_id": "target"\n}\n');
  const expectedResponses = [
    {
      schema_version: 'ascension.provider-session.policy-owner-view.v1',
      operation: 'current',
      value: { run_id: runId, revision: 1, active: null, history: [], proposals: [] },
      effect_class: 'local_metadata_only',
      inference_calls: 0,
      game_effects: 0,
    },
    command('import', 2, policyDigest),
    command('propose', 3, null, proposalDigest),
    command('approve', 4),
    command('adopt', 5, targetDigest),
    command('adopt', 6, policyDigest),
  ];
  const calls = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url, options) => {
    calls.push({ url: String(url), options, body: options.body });
    return response(expectedResponses[calls.length - 1]);
  };
  try {
    const view = await client.getProviderSessionPolicy(runId, ownerToken);
    assert.equal(view.value.revision, 1);
    await client.importProviderSessionPolicy(runId, ownerToken, 1, sourceBytes.buffer.slice(
      sourceBytes.byteOffset,
      sourceBytes.byteOffset + sourceBytes.byteLength,
    ));
    await client.proposeProviderSessionPolicy(
      runId,
      ownerToken,
      'proposal.client',
      policyDigest,
      2,
      targetBytes.buffer.slice(
        targetBytes.byteOffset,
        targetBytes.byteOffset + targetBytes.byteLength,
      ),
    );
    await client.approveProviderSessionPolicy(
      runId,
      ownerToken,
      'proposal.client',
      proposalDigest,
      'approval.client',
      3,
    );
    await client.adoptProviderSessionPolicyProposal(
      runId,
      ownerToken,
      'proposal.client',
      proposalDigest,
      'approval.client',
      4,
    );
    await client.adoptImportedProviderSessionPolicy(runId, ownerToken, policyDigest, 5);
  } finally {
    globalThis.fetch = originalFetch;
  }

  assert.equal(calls.length, 6);
  assert.deepEqual(calls.map(({ url, options }) => {
    const parsed = new URL(url, 'http://console.test');
    assert.equal(parsed.origin, 'http://console.test');
    return [options.method || 'GET', parsed.pathname, parsed.search];
  }), [
    ['GET', `/v1/workflow-runs/${runId}/provider-session-policy`, ''],
    ['POST', `/v1/workflow-runs/${runId}/provider-session-policy/import`, '?expected_revision=1'],
    ['POST', `/v1/workflow-runs/${runId}/provider-session-policy/proposals/proposal.client`, `?source_sha256=${policyDigest}&expected_revision=2`],
    ['POST', `/v1/workflow-runs/${runId}/provider-session-policy/proposals/proposal.client/approve`, '?expected_revision=3'],
    ['POST', `/v1/workflow-runs/${runId}/provider-session-policy/proposals/proposal.client/adopt`, '?expected_revision=4'],
    ['POST', `/v1/workflow-runs/${runId}/provider-session-policy/adoptions`, '?expected_revision=5'],
  ]);
  assert.deepEqual(Buffer.from(calls[1].body), sourceBytes);
  assert.deepEqual(Buffer.from(calls[2].body), targetBytes);
  assert.deepEqual(JSON.parse(calls[3].body), {
    schema_version: commandSchema,
    proposal_sha256: proposalDigest,
    approval_ref: 'approval.client',
  });
  assert.deepEqual(JSON.parse(calls[4].body), JSON.parse(calls[3].body));
  assert.deepEqual(JSON.parse(calls[5].body), {
    schema_version: commandSchema,
    policy_sha256: policyDigest,
  });
  for (const call of calls) {
    assert.equal(call.options.credentials, 'same-origin');
    assert.equal(call.options.cache, 'no-store');
    assert.equal(call.options.headers.Authorization, `Bearer ${ownerToken}`);
    assert.equal(call.url.includes(ownerToken), false);
  }
});

test('saved-policy client rejects a response for another run and oversized upload before fetch', async () => {
  const client = await loadClient();
  const originalFetch = globalThis.fetch;
  let fetchCount = 0;
  globalThis.fetch = async () => {
    fetchCount += 1;
    return response({
      schema_version: 'ascension.provider-session.policy-owner-view.v1',
      operation: 'current',
      value: { run_id: 'run.foreign', revision: 1, active: null, history: [], proposals: [] },
      effect_class: 'local_metadata_only',
      inference_calls: 0,
      game_effects: 0,
    });
  };
  try {
    await assert.rejects(
      client.getProviderSessionPolicy(runId, ownerToken),
      (error) => error.status === 409 && error.code === 'provider_session_policy_run_mismatch',
    );
    await assert.rejects(
      client.importProviderSessionPolicy(runId, ownerToken, 1, new ArrayBuffer(1024 * 1024 + 1)),
      (error) => error.status === 400 && error.code === 'provider_session_policy_upload_bound',
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
  assert.equal(fetchCount, 1, 'oversized uploads must fail before network access');
});
