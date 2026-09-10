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

The checked-in browser and CLI demo are offline synthetic evidence. A local Chromium run exercised
the browser fixture at desktop (`1440x1000`) and narrow (`375x800`) viewports with reduced motion,
keyboard comparison, same-origin-only requests, zero browser persistence, and no normal-flow
console or page errors. It also exercises typed pin/restore operations and denial probes, injects
an HTML payload into untrusted provider/component fields, and confirms text-only rendering with no
markup nodes, script execution, or external request. Its result and
screenshots are in `docs/evidence/browser-ui-20260910.json` and the two adjacent PNG files. The
run covers the normal synthetic flow; filesystem spies and native storage enforcement remain
separate evidence limits.

The integrated browser audit runs `context-console integrated-demo 0` as a loopback server and
loads `/web/` from that process. The server creates the synthetic producer projection, passes the
same bytes through `MemoryCapture`, retains them in `Store`, and routes the browser's declared
`/demo/*` artifacts through `ReadApi`. The audit asserts seven events, two captured records, at
least three API projections, zero provider/game/external calls, and the same adversarial,
keyboard, reduced-motion, persistence, and narrow-layout conditions. Its result and screenshots
are the three `integrated-browser-*` artifacts in `docs/evidence/`.

The companion harness branch records a bounded baseline/successor differential run for Astra and
Ollama with synthetic fake downstreams; its machine-readable result is
`docs/evidence/context-capture-fidelity-20260910.json`. That evidence covers the bridge handoff and
failure cases only. It does not establish a real provider or game receipt, integrated
producer-to-browser execution, or independent security review.
