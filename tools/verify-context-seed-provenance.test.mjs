import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

test('seed classification is explicit and producer qualification fails closed', () => {
  const valid = spawnSync(process.execPath, ['tools/verify-context-seed-provenance.mjs'], {encoding: 'utf8'});
  assert.equal(valid.status, 0);
  const required = spawnSync(process.execPath, ['tools/verify-context-seed-provenance.mjs', '--require-producer-qualification'], {encoding: 'utf8'});
  assert.notEqual(required.status, 0);
  assert.match(required.stderr, /producer qualification unavailable/);
});

test('classification rejects changed historical reference and changed admission evidence', () => {
  const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'context-seed-provenance-'));
  try {
    fs.mkdirSync(path.join(fixture, 'tools'));
    fs.mkdirSync(path.join(fixture, 'contract-artifact'));
    fs.copyFileSync('tools/verify-context-seed-provenance.mjs', path.join(fixture, 'tools/verify-context-seed-provenance.mjs'));
    fs.cpSync('contract-artifact/context-inspection-v1', path.join(fixture, 'contract-artifact/context-inspection-v1'), {recursive: true});
    const run = () => spawnSync(process.execPath, ['tools/verify-context-seed-provenance.mjs'], {cwd: fixture, encoding: 'utf8'});
    assert.equal(run().status, 0);
    const file = path.join(fixture, 'contract-artifact/context-inspection-v1/provenance-status.json');
    const original = fs.readFileSync(file, 'utf8');
    fs.writeFileSync(file, JSON.stringify({...JSON.parse(original), historical_revision: '0'.repeat(40)}));
    assert.match(run().stderr, /does not match the historical manifest/);
    fs.writeFileSync(file, original);
    const readme = path.join(fixture, 'contract-artifact/context-inspection-v1/README.md');
    fs.writeFileSync(readme, 'Now claiming an adopted upstream contract');
    assert.match(run().stderr, /classification evidence changed/);
  } finally {
    fs.rmSync(fixture, {recursive: true, force: true});
  }
});
