# Phase 4 implementation handoff (fixture/source scope)

Date: 2026-09-11. The target implementation source baseline is `1508164`.
The companion harness baseline is `d82ccb6`. Subsequent target commits are test-helper, evidence and documentation updates.

| Seam | Owner and implementation | Evidence | Limit |
| --- | --- | --- | --- |
| Frozen capture, renderer and Phase 2 held boundary | companion harness `context_capture`, `context_control`, `context_memory` and Exo modules | inherited Phase 1–3 tests and handoff | live provider/game remains unverified |
| Persistent-session policy and identities | harness `provider_session::{types/*,broker*}` | `tests/provider_session.rs`, `tests/provider_session_snapshot.rs` | bounded metadata snapshot restore; prepared/native state compatibility unverified |
| Bounded native framing | harness `provider_session::{protocol*,transport*}` | compiled `provider-session-peer` test; JSON-RPC 2.0 strict-frame fixtures | fixture peer is not proof of native-binary compatibility |
| Client API and CLI | target `provider_session.rs`, `run_phase4_cli`, additive OpenAPI seed | `tests/phase4_session.rs`; `phase4-cli` | target owns no native state or credentials |

The native three-level Luna/max preflight was attempted with a real depth-1 lead observed at
`gpt-5.6-luna`/`max`. That session exposed no child-spawn, reservation, messaging, or close
operation, so depth-2/depth-3 ancestry could not be established and no delegated implementation
claim is made. No real provider, game, deployment, merge, or release was performed. The
implementation branch is pushed to GitHub as `phase4/persistent-provider` in both companion
repositories. The fixture lane is intentionally `fixture_only`. A bounded native fake-upstream
lane with durable restart/read, fork, compact/start endpoint probes, and a restricted feature/config
probe is recorded in the Phase 4 evidence directory. The restricted probe removed forbidden
executable tool classes but retained `request_user_input`; complete native hardening and full
native/live-provider capabilities remain `unverified`. A fully disabled native fake profile forwarded
zero tool definitions and ignored a forged `exec` call without a server request; a Linux tmpfs lane
kept regular native state files volatile. Runtime developer instructions, missing OS egress proof and
encrypted persistent state remain explicit limits.

Delivery is tracked in [target PR #12](https://github.com/AI-Ascension/ascension-context-console/pull/12)
and [companion PR #87](https://github.com/AI-Ascension/sts2-harness/pull/87); both were merged to
`main` in `e4ab38a` and `544605b` after their required checks passed.

The harness snapshot lane persists only the bounded local metadata journal: operation
idempotency, epochs, maintenance records and redacted history projections. Admission enforces
validated expiry and aggregate local byte bounds. Restore rotates to a fresh owner epoch, applies
retirement tombstones before exposing operations/history, preserves in-flight turns as unknown and
held, and rejects checked restores whose scope/policy/profile drifts; it never serializes prepared
turn bytes or auto-resumes an in-flight turn. The fixture capability reports encrypted state as
unverified and native encrypted-state durability remains unverified. The harness rejects inspect
or enabled native profiles until that boundary is independently verified.

The broker journal also has explicit `volatile` and `encrypted_persistent` adapters. The latter
authenticates only the bounded metadata snapshot and uses private, owner-checked path validation;
it does not encrypt or contain native Codex state, rollout files, WAL/log files or temporary files.
The owned stdio transport clears ambient environment roots and binds conventional home/config/cache/
temporary variables to its approved state root; native OS containment remains unverified. Startup
scans the private state tree against a 256 MiB bound and fails closed on unsafe entries, with
explicit entry/depth bounds. Before each allowlisted request and after each response it rescans the
tree; an overage fences and terminates the owned child, covered by a compiled-fixture growth test.
The compiled fixture reports this binding during initialization, while installed-native precedence
and runtime behavior remain unverified.
Configuration accepts only the approved `OPENAI_API_KEY` inherited secret name; unrelated credential,
endpoint and path variables are rejected.
Finite retention is enforced at admission and restore: policy validation caps completed turns at 128
and scope lifetime at 24 hours; history refresh, turn completion and snapshot restore apply the
selected turn and aggregate-byte limits; fork-created evaluation bindings and combined fork/
compaction maintenance consume the four-candidate and two-job quotas.

Pending candidate, reconnect and compaction completions are fenced after retirement, while pending
fork/compaction maintenance is invalidated on owner rotation or crash recovery; the companion
retirement regression lane covers these no-resurrection transitions.
History cursors are binding- and scope-tagged and carry the current history epoch; prepared turns
carry the broker auth and revocation epochs, so cross-binding, stale-history, and stale-authorization
replays are rejected.

The first resumed submission revalidates the entire approval dependency vector against the current
binding rather than trusting construction time: owner/auth/session/history/compaction/revocation
epochs, profile and continuity digests, and dependency identity must all still match the prepared
record, which is itself re-validated at that boundary. A history refresh, compaction completion,
dependency change, owner rotation or crash recovery therefore invalidates the exact approved bytes.

An unapproved automatic context transform observed during an in-flight turn is fenced by
`fence_automatic_transform`: the attempt is retained as unknown, the binding is held, and the
history epoch advances so the late result and any stale prepared input can never regain authority.
Wire-level native transform-notification detection remains unverified; only the broker fence is
directly exercised.

The encrypted metadata journal is now read through a no-follow descriptor and the resulting file
metadata is rechecked on that descriptor, closing the path-swap window between path validation and
read. Its load test proves a symlink redirection is rejected.
