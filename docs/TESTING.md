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
redaction. The enabled corpus is synthetic; live adapter, native game, migration/restore process,
and deployment lanes are recorded with their exact evidence class in the Phase 3 evidence report.

`phase2_cli` includes direct reducer checks and a compiled-process workflow covering init, metadata
capabilities, redacted eligible items, typed stdin draft editing, exploratory/applicable preview,
pause, commit, and explicit resume. The process test verifies the
`ascension.context-control.cli-result.v1` envelope and exercises separate command invocations
against the durable fixture without provider or game effects. CLI stdin is capped at 16 KiB and
uses the same duplicate-key/depth checks as the HTTP path; shell-facing exit classes are documented
in [docs/API.md](API.md).

The checked-in browser and CLI demo are offline synthetic evidence. The current Chromium audit was
attempted for desktop and narrow viewports but could not start because the cached browser is
missing `libglib-2.0.so.0`; no page assertions or screenshots were produced at this revision.
Historical browser artifacts remain in `docs/evidence/` for comparison only and are not counted as
current gate evidence.

The integrated browser audit still runs `context-console integrated-demo 0` as a loopback server
when the required browser libraries are available. Its intended assertions cover the synthetic
producer projection, `MemoryCapture`, `Store`, declared `/demo/*` artifacts, zero provider/game/
external calls, adversarial text, keyboard behavior, persistence, and narrow layout. The current
environment did not reach those assertions.

The companion harness branch records a bounded baseline/successor differential run for Astra and
Ollama with synthetic fake downstreams; its machine-readable result is
`docs/evidence/context-capture-fidelity-20260910.json`. That evidence covers the bridge handoff and
failure cases only. It does not establish a real provider or game receipt, integrated
producer-to-browser execution, or independent security review.
