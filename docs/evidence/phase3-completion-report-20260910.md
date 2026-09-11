# Phase 3 completion report

Date: 2026-09-11. This is the bounded implementation and evidence record for the additive
`ascension.context-memory.*.v1` surface on the `phase2/context-editing` branch, merged to target
`main` by PR #3. It is not a
product-readiness or live-runtime claim. Target implementation code is pinned to
`8e52da837ae0a23cea18d7cd3d5164765911e6b6`; the companion harness implementation is pinned to
`3cb72968bee8943e89158a28cec27d7b88e1ce91`.

The target extends the existing Phase 1/2 checkout with the eleven versioned context-memory
contracts and synthetic fixtures, a permissioned `/v3/memory` facade, bounded `phase3-cli`
capabilities/status/search inspection, integrated-demo routing, inert browser rendering, operator
documentation, and route/CLI tests. The target owns no corpus, provider, game, process, URL, or
summary scheduler. Because no harness projection is attached in this checkout, the shipped target
CLI and integrated demo start memory disabled; the capability response exposes only the operation
the facade can serve, and unattached retrieval/compaction/policy lanes remain `unverified` or
`unsupported`. Search reports `projection_unavailable` rather than inventing results.

The companion harness implements bounded policy units for scoped admission, occurrence identity,
causal and revocation fences, deterministic Unicode lexical retrieval, exact and explicitly lossy
extracts, review-required summary jobs, immutable review revisions, critical-fact checks, whole
rendered-input selection, Phase 2 binding records, held approvals, explicit one-shot resume,
generation-fenced map bundles, role ACLs, redacted telemetry, encrypted SQLite persistence,
revocation-first restore, generation-aware cache invalidation, finite retention, resumable migration,
downgrade fences, immutable per-attempt usage, held-out evaluation records, and an executable fake
peer. The harness also provides bounded CLI, peer, measurement, and executable adapter-demo binaries.
The checked-in adapter binds a reviewed selection to the target's Phase 2 prepared manifest and
atomic SQLite memory-binding record in an offline synthetic fixture; it does not attach a production
projection or claim live provider/game behavior.

The 90-row requirement CSV and 90-row failure matrix are synchronized to these revisions. Their
status classes are deliberately evidence classes: local deterministic tests and source-backed
records are `executed_synthetic`; lanes that have an implementation seam but no current proof are
`unverified`; unavailable mandatory integrations are `blocked`. The current matrix records
`executed_synthetic` for 89 policy, persistence, CLI, fake-peer, oracle, map, review, resume,
migration, measurement, browser, and adapter-process rows, with 1 blocked native-review row. No row
is silently skipped or labeled product verified.

## Gates

The following commands passed with exit status 0 on the target implementation revision:

```text
cargo run --locked --package repo-policy -- --strict
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked -- --test-threads=1
PHASE3_TARGET_BINARY=.../phase2-target/target/debug/context-console PHASE3_FAKE_PEER_BINARY=.../phase2-harness/target/debug/context-memory-peer ./target/debug/phase3-adapter-demo
```

The package verifier also passed from the package directory:

```text
../.phase3-venv/bin/python tools/verify_package.py --full
```

That verifier checked 90 requirements, 90 failure scenarios, 100 JSON documents, 11 new schemas,
15 valid fixtures, 47 rejected invalid fixtures, 27 semantic negative fixtures, 18 API operations,
and the finite reference model. It explicitly reports package validation only and is not product
proof.

The current integrated and Phase 2 browser audits passed with Playwright 1.63.0 / Chromium
153.0.8010.12 at target revision `f22296225c9e6b5a36004d1d689f9e27384920ba`. They recorded zero
external requests, zero provider calls, zero game launches, no browser persistence, inert
adversarial text, no forbidden manifest fetch, no narrow overflow, and the complete typed
Phase 2 pause/commit/resume flow. The exact JSON and PNG artifacts are checked in under
`docs/evidence/`.

## Limits and blockers

Target PR #3 was merged after its `policy` and `Rust quality gates` checks passed; the squash merge
commit is `114b3e5ae28cd60d9dafc421711dee859b602c4d`. Harness PR #70 was also merged externally;
its merge commit is `8d771f128bc0ba13071063425c9a852bac2c40c1`. No release, deployment, live provider
call, game launch, credential use, or unrelated write was performed. The latest native recheck
observed Codex `0.153.4`, but `codex1 agents --no-alt-screen` failed because the managed standalone
binary is absent and the read-only Herdr `agent.start` surface exposes no model, parent, depth, or
reservation fields. The target's default facade still has no attached persistent
projection; the explicit adapter proof is an offline synthetic path. The remaining mandatory
evidence is the independent native three-level reviewer. Synthetic adapter, process, and network
tripwire evidence cannot establish provider quality, native game/action effects, production storage
operations, browser compatibility, or live deployment. The handoff does not claim that all mandatory
requirements are product-verified; the native three-level reviewer remains blocked.
