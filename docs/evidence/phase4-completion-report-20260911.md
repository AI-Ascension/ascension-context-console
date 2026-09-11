# Phase 4 completion and evidence report

Date: 2026-09-11. This report describes the current reviewable fixture/source lane. It does not
claim native Codex-binary compatibility, live provider/game behavior, deployment, or completion of
the unavailable native three-level orchestration gate. The per-requirement ledger is
[`phase4-requirement-evidence-20260911.csv`](phase4-requirement-evidence-20260911.csv).

## Delivered source

| Repository | Branch and commit | Publication | Scope |
| --- | --- | --- | --- |
| [ascension-context-console](https://github.com/AI-Ascension/ascension-context-console/tree/phase4/persistent-provider) | `phase4/persistent-provider` implementation source baseline `b502c77` | committed and pushed; this report update is documentation-only; not merged/released/deployed | typed client route, integrated demo, browser surface, capability disclosure |
| [sts2-harness](https://github.com/AI-Ascension/sts2-harness/tree/phase4/persistent-provider) | `phase4/persistent-provider` at `d9849aa` | committed and pushed; not merged/released/deployed | broker ownership, lifecycle fencing, strict JSON-RPC fixture transport, bounded metadata snapshot restore |

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

The harness snapshot lane persists only the bounded local metadata journal: operation
idempotency, epochs, maintenance records and redacted history projections. Local admission enforces
validated expiry and bounded aggregate history/prepared bytes. Restore rotates to a fresh owner
epoch, applies retirement tombstones before operations/history are exposed, preserves in-flight
turns as unknown and held, and offers a checked restore that rejects scope/policy/profile drift;
it never serializes prepared turn bytes or auto-resumes an in-flight turn. Native encrypted-state
durability remains unverified.

The journal has an explicit volatile adapter and a broker-owned encrypted-persistent adapter. The
encrypted adapter authenticates only this bounded metadata envelope and uses fail-closed absolute,
owner-checked, restrictive path validation with atomic replacement; it is not a claim about native
Codex storage or auxiliary-file containment.
The owned stdio transport clears ambient environment roots, binds conventional home/config/cache/
temporary variables to its approved state root, and scans that private tree against a 256 MiB quota.
The walk fails closed on unsafe entries and is repeated before each allowlisted request and after
each response; an overage fences and terminates the owned child, with a compiled-fixture growth test.
These are source/fixture guarantees, not proof of installed-native precedence or runtime behavior.
Only the approved `OPENAI_API_KEY` inherited secret name is accepted; unrelated credential,
endpoint and path variables are rejected.
Policy validation also caps scope lifetime at 24 hours and completed turns at 128. History refresh,
turn completion and snapshot restore enforce turn and aggregate-byte bounds; fork-created evaluation
bindings and combined fork/compaction maintenance are bounded to four candidates and two jobs.

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
harness: cargo test --locked -p sts2-harness --test provider_session --test provider_session_safety --test provider_session_snapshot --test provider_session_store
harness: cargo test --locked -p sts2-harness --lib provider_session::transport::config::tests::process_environment_is_bound_to_state_root
harness: cargo test --locked -p sts2-harness --lib provider_session::transport::io::tests::state_quota_scan_is_bounded_and_fail_closed
harness: cargo test --locked -p sts2-harness --lib provider_session::transport::tests::runtime_state_growth_fences_and_stops_owned_peer
harness: cargo test --locked -p sts2-harness --lib provider_session::transport::tests::forbidden_native_method_is_rejected_before_write
target: node --check web/app.js && node --check tools/integrated_browser_audit.cjs
target: cargo test --locked -p context-service --test phase4_session
harness: cargo test --locked -p sts2-harness --test provider_session --test provider_session_safety --test provider_session_snapshot
```

The harness full suite passed with 181 tests and one ignored test in the large execution-store
group plus the remaining workspace groups; the explicit `TMPDIR` was required because the default
`/tmp` tmpfs exhausted while constructing oversized SQLite fixtures. The target full suite passed.
Loopback probes against the built integrated demo returned capabilities/list/candidate success and
403 for a wrong-origin candidate write; all reported zero native calls and zero game effects. The
exact probe record is [`phase4-session-loopback-20260911.json`](phase4-session-loopback-20260911.json).

The current browser audit ran with Playwright 1.63.0 and Chromium 153.0.8010.12. It exercised the
Phase 1–3 panels plus the Phase 4 capabilities/list/candidate flow, wrong-origin rejection, reduced
motion, narrow layout, adversarial text, zero external requests, and zero browser persistence. The
fresh JSON/PNG evidence is recorded in `docs/evidence/integrated-browser-ui-20260910.json` and the
same run's 20260911 PNG artifacts. Full native-profile and live-provider/game checks remain
unverified.

## Orchestration and native limits

The latest native preflight observed a real depth-1 `gpt-5.6-luna`/`max` lead. Its native callable
registry exposed no child-spawn, reservation, messaging or close operation, so depth-2 coordinator
and depth-3 leaf ancestry could not be verified. No delegated implementation claim is made. The
installed Codex 0.154.0 binary and generated App Server schema were inspected without inference;
their digests and reviewed method names are recorded in
[`NATIVE_CAPABILITIES.md`](../phase4/NATIVE_CAPABILITIES.md). The bounded metadata smoke and the
loopback fake-upstream/two-turn plus durable restart evidence are recorded in
[`phase4-native-isolated-smoke-20260911.json`](phase4-native-isolated-smoke-20260911.json) and
[`phase4-native-fake-conformance-20260911.json`](phase4-native-fake-conformance-20260911.json).
The fake lane used no real provider credentials, observed native tool-definition forwarding, and also
recorded bounded durable-fork and compact/start endpoint probes. A restricted configuration probe
removed the forbidden executable tool classes but retained `request_user_input`; it therefore does not
establish complete hardening, clean rehydration or compaction semantics, OS-level egress trap, or full
provider compatibility. A provider-generated `request_user_input` callback attempt emitted no native
server request and repeatedly reported the tool unavailable in Default mode. The read-only migration
dry run found no eligible sessions in an empty disposable root, so persisted-state migration remains
unverified. A fully disabled native fake profile forwarded zero tool definitions and ignored a forged
`exec` function-call without a server request; a Linux tmpfs variant kept all regular native state
files volatile and wrote no `history.jsonl`. Those lanes strengthen the explicit candidate profiles,
but the runtime developer envelope, missing bubblewrap/OS egress trap and lack of encrypted persistent
storage keep complete native hardening and cross-platform guarantees unverified.

## External effects and handoff

No authenticated real-provider inference was executed and no game was launched. An earlier
exploratory native start reached the provider websocket without credentials and received an HTTP
401; the bounded fake-upstream lane sent turns only to a local loopback server, while the metadata
smoke used a loopback-refused proxy and sent no turn. No credential/account/configuration was
changed, and no remote repository was merged, released or deployed. The branches are ready for
independent review. Activation still requires an authorized isolated native binary/fake-upstream profile,
successful native depth preflight, browser dependencies for a fresh audit, and separately approved
live-provider/game checks. Until those gates pass, the only enabled profile is `fixture_only`.
