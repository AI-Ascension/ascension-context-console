import fs from 'node:fs';

const status = JSON.parse(fs.readFileSync('contract-artifact/context-inspection-v1/provenance-status.json', 'utf8'));
const manifest = JSON.parse(fs.readFileSync('contract-artifact/context-inspection-v1/manifest.json', 'utf8'));
const seedReadme = fs.readFileSync('contract-artifact/context-inspection-v1/README.md', 'utf8');
const openapi = JSON.parse(fs.readFileSync('contract-artifact/context-inspection-v1/read-api.openapi.json', 'utf8'));
if (status.schema_version !== 1 || status.classification !== 'original_local_seed'
  || status.producer_qualification !== 'unverified_not_applicable'
  || status.historical_repository !== 'AI-Ascension/sts2-harness'
  || !/^[0-9a-f]{40}$/.test(status.historical_revision)
  || !Array.isArray(status.evidence) || status.evidence.length < 3
  || !Array.isArray(status.required_for_producer_qualification) || status.required_for_producer_qualification.length !== 3) {
  throw new Error('invalid context seed provenance status');
}
if (manifest.source_repository !== status.historical_repository || manifest.source_revision !== status.historical_revision) {
  throw new Error('seed classification does not match the historical manifest reference');
}
if (!seedReadme.includes('original implementation seeds') || !seedReadme.includes('not existing runtime interfaces')
  || openapi.info?.version !== '0.1.0-proposed') {
  throw new Error('seed classification evidence changed; review ownership and admission before updating this receipt');
}
if (process.argv.includes('--require-producer-qualification')) {
  throw new Error('producer qualification unavailable: original local seeds have no adopted producer source paths');
}
process.stdout.write('Context seed provenance: local integrity required; producer qualification unverified/not applicable.\n');
