# Testing

The required local gates are:

```text
cargo run --locked --package repo-policy -- --strict
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Reader tests cover bounds, duplicate fields, immutable completeness, mapping references, event
allowlisting, and explicit unavailable measurements. Store tests cover idempotent ingest, conflicting
bytes, expiry, revocation, scope, bounded event pages, and read-only API routing. Capture tests
cover off/metadata/memory/private contracts, queue overflow, authenticated encryption, and
metadata-only telemetry. Harness tests cover the direct Exo session and generic provider route,
including prepared, completed, and indeterminate write states; bridge unit tests cover the final
Astra and Ollama input component seams.

Phase 2 control tests cover protected-item and draft-version rejection, objective authorization,
typed pin/exclude/restore operations, exploratory versus applicable previews, pause/commit/resume
ordering, idempotent receipts, command-window expiry, obsolete-plan and changed-boundary fencing,
stop dominance, deactivation, and journal tamper recovery. The companion harness tests cover
legacy byte parity, enabled managed context, exact prepared bytes through Exo, enabled Ollama
projection, controller recovery, and fail-closed invalid UTF-8/expiry/pin validation.

The target `phase2_durable` tests cover encrypted journal reopen, wrong-key/tamper rejection,
transaction rollback with outbox atomicity, additive schema repair with retained Phase 1 bytes,
legacy active-state refusal, and immutable snapshot backup.

Phase 3 target tests cover the closed `/v3/memory` query, separate search/review permissions,
disabled capability disclosure, projection-unavailable responses, and the compiled `phase3-cli`
help/capabilities/search commands. The target facade remains provider/game/process free. Companion
harness tests cover causal scope and cutoff filtering, deterministic lexical ties, exact source
citations, an independent hand-labelled oracle, fake summary input capture and unknown outcomes,
independent review/admission, whole rendered selection budgets, approval held/resume fencing,
encrypted SQLite restore/revocation, map generation checks, least-privilege roles, and telemetry
redaction. The enabled corpus is synthetic. The checked-in `phase3-adapter-demo` proves the offline
target↔harness process seam, exact Phase 2 binding, and zero-effect tripwires; live provider, native
game, migration/restore process, and deployment lanes remain separate evidence classes in the Phase 3
evidence report.

`phase2_cli` includes direct reducer checks and a compiled-process workflow covering init, metadata
capabilities, redacted eligible items, typed stdin draft editing, exploratory/applicable preview,
pause, commit, and explicit resume. The process test verifies the
`ascension.context-control.cli-result.v1` envelope and exercises separate command invocations
against the durable fixture without provider or game effects. CLI stdin is capped at 16 KiB and
uses the same duplicate-key/depth checks as the HTTP path; shell-facing exit classes are documented
in [docs/API.md](API.md).

The checked-in browser and CLI demo are offline synthetic evidence. The current Chromium audit
passed at desktop (`1440x1000`) and narrow (`375x800`) viewports with reduced motion, keyboard
comparison, same-origin-only requests, zero browser persistence, no normal-flow page errors,
adversarial text rendered inertly, and no forbidden manifest path requests. Its result and
screenshots are in `docs/evidence/integrated-browser-ui-20260910.json` and the adjacent PNG files.

The integrated browser audit runs `context-console integrated-demo 0` as a loopback server. The
current result asserts the synthetic producer projection, `MemoryCapture`, `Store`, declared
`/demo/*` artifacts, zero provider/game/external calls, adversarial text, keyboard behavior,
persistence, and narrow layout. The separate Phase 2 browser audit also passed the typed
pin/restore, pause/commit/resume, denial and conflict probes; its result is in
`docs/evidence/phase2-browser-ui-20260910.json`.

The companion harness branch records a bounded baseline/successor differential run for Astra and
Ollama with synthetic fake downstreams; its machine-readable result is
`docs/evidence/context-capture-fidelity-20260910.json`. That evidence covers the bridge handoff and
failure cases only. It does not establish a real provider or game receipt, integrated
producer-to-browser execution, or independent security review.
