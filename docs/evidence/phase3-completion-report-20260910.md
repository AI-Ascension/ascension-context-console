# Phase 3 completion report

Date: 2026-09-11. This is the bounded implementation and evidence record for the additive
`ascension.context-memory.*.v1` surface on the `phase2/context-editing` draft branch. It is not a
product-readiness or live-runtime claim. Target implementation code is pinned to
`b21083719fb825be83088c9a6ab09531463a1a72`; the companion harness implementation is pinned to
`f8213e90388a77f5d893a40b37498aaf5eb78265`.

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
peer. The harness also provides bounded CLI, peer, and measurement binaries. These are deterministic
component and synthetic-boundary lanes; they are not the missing target↔harness Phase 2 adapter.

The 90-row requirement CSV and 90-row failure matrix are synchronized to these revisions. Their
status classes are deliberately evidence classes: local deterministic tests and source-backed
records are `executed_synthetic`; lanes that have an implementation seam but no current proof are
`unverified`; unavailable mandatory integrations are `blocked`. The current matrix records
`executed_synthetic` for 77 implemented policy, persistence, CLI, fake-peer, oracle, map, review,
resume, migration, and measurement rows, with 7 explicit unverified browser/current-adapter rows
and 6 blocked real-process, end-to-end, and native-review rows.

## Gates

The following commands passed with exit status 0 on the target implementation revision:

```text
cargo run --locked --package repo-policy -- --strict
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked -- --test-threads=1
```

The package verifier also passed from the package directory:

```text
../.phase3-venv/bin/python tools/verify_package.py --full
```

That verifier checked 90 requirements, 90 failure scenarios, 100 JSON documents, 11 new schemas,
15 valid fixtures, 47 rejected invalid fixtures, 27 semantic negative fixtures, 18 API operations,
and the finite reference model. It explicitly reports package validation only and is not product
proof.

The current integrated and Phase 2 browser audits were attempted with Playwright 1.63.0. Chromium
exited before launch because `libglib-2.0.so.0` is unavailable in the environment, so no current
browser assertions or screenshots are claimed. Historical browser artifacts from an earlier target
revision remain clearly out of the current gate record.

## Limits and blockers

No merge, release, deployment, live provider call, game launch, credential use, or unrelated write
was performed. The target has no attached persistent projection or summary adapter. The following
mandatory evidence remains unavailable or blocked: atomic cross-repository Phase 2 binding, a real
summary process/tool tripwire, target↔harness end-to-end process and network tripwires, and an
independent native three-level reviewer. A durable local policy proof cannot establish provider
quality, native game/action effects, production storage operations, browser compatibility, or live
deployment. The draft PRs therefore remain open and draft, and the handoff does not claim that all
mandatory requirements are product-verified.
