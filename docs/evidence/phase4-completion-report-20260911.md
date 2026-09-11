# Phase 4 completion and evidence report

Date: 2026-09-11. This report describes the current reviewable fixture/source lane. It does not
claim native Codex-binary compatibility, live provider/game behavior, deployment, or completion of
the unavailable native three-level orchestration gate. The per-requirement ledger is
[`phase4-requirement-evidence-20260911.csv`](phase4-requirement-evidence-20260911.csv).

## Delivered source

| Repository | Branch and commit | Publication | Scope |
| --- | --- | --- | --- |
| [ascension-context-console](https://github.com/AI-Ascension/ascension-context-console/tree/phase4/persistent-provider) | `phase4/persistent-provider` at `ba7388c` (feature source `96bdf9e`) | committed and pushed; not merged/released/deployed | typed client route, integrated demo, browser surface, capability disclosure |
| [sts2-harness](https://github.com/AI-Ascension/sts2-harness/tree/phase4/persistent-provider) | `phase4/persistent-provider` at `5d664f3e0bfd3bf3303066947ba0021698b7c3a6` | committed and pushed; not merged/released/deployed | broker ownership, lifecycle fencing, strict JSON-RPC fixture transport |

The working trees were fetched and fast-forward synchronized after push. No unrelated changes were
reset or overwritten.

## Implementation and capability status

The fixture transport now emits and accepts bounded JSON-RPC 2.0 line objects with strict duplicate
key/depth checks, a reviewed method allowlist, explicit initialization, notification deduplication,
and fail-closed denial of server-initiated requests. The reviewed method set is `initialize`,
`thread/start`, `thread/read`, `turn/start`, `turn/interrupt`, `thread/fork`, and
`thread/compact/start`; retirement is local broker state because the inspected generated schema
does not expose `thread/retire`.

The target's authenticated `/v1/runs/{run_id}/provider-sessions*` projection is additive and
fixture-only. The integrated loopback demo serves capabilities, scoped listings, history/operation
metadata and an explicit evaluation-candidate action. Candidate writes require the synthetic
session bearer and same-origin CSRF proof. The UI reports `fixture_only`, `compiled_peer`,
`owned_stdio`, disabled tools/ambient history, zero native calls and zero game effects. It never
accepts raw RPC methods, provider credentials, game actions or automatic resume.

## Verification

Passed on the committed source:

```text
target: cargo fmt --all -- --check
target: cargo check --locked --workspace --all-targets
target: cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
target: cargo run --locked --package repo-policy -- --strict
target: cargo test --locked --workspace --all-targets --all-features --quiet
harness: cargo fmt --all -- --check
harness: cargo check --locked --workspace --all-targets
harness: cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
harness: cargo run --locked --package repo-policy -- --strict
harness: TMPDIR=<dedicated persistent temp directory> cargo test --locked --workspace --all-targets --all-features --quiet
target: node --check web/app.js && node --check tools/integrated_browser_audit.cjs
target: cargo test --locked -p context-service --test phase4_session
harness: cargo test --locked -p sts2-harness --test provider_session --test provider_session_safety
```

The harness full suite passed with 181 tests and one ignored test in the large execution-store
group plus the remaining workspace groups; the explicit `TMPDIR` was required because the default
`/tmp` tmpfs exhausted while constructing oversized SQLite fixtures. The target full suite passed.
Loopback probes against the built integrated demo returned capabilities/list/candidate success and
403 for a wrong-origin candidate write; all reported zero native calls and zero game effects.

The current browser audit script is syntax-checked but was not executed because the Playwright and
Chromium paths used by the historical evidence are unavailable in this environment. The older
browser artifacts therefore are not treated as proof for this revision. Native-binary execution
and live-provider/game checks remain unverified.

## Orchestration and native limits

The latest native preflight observed a real depth-1 `gpt-5.6-luna`/`max` lead. Its native callable
registry exposed no child-spawn, reservation, messaging or close operation, so depth-2 coordinator
and depth-3 leaf ancestry could not be verified. No delegated implementation claim is made. The
installed Codex 0.154.0 binary and generated App Server schema were inspected without inference;
their digests and reviewed method names are recorded in
[`NATIVE_CAPABILITIES.md`](../phase4/NATIVE_CAPABILITIES.md). Starting that binary against
personal/default state was not authorized and no isolated fake-upstream lane was available.

## External effects and handoff

No real provider was called, no game was launched, no credential/account/configuration was changed,
and no remote repository was merged, released or deployed. The branches are ready for independent
review. Activation still requires an authorized isolated native binary/fake-upstream profile,
successful native depth preflight, browser dependencies for a fresh audit, and separately approved
live-provider/game checks. Until those gates pass, the only enabled profile is `fixture_only`.
