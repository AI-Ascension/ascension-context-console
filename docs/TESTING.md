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

The checked-in browser and CLI demo are offline synthetic evidence. A local Chromium run exercised
the browser fixture at desktop (`1440x1000`) and narrow (`375x800`) viewports with reduced motion,
keyboard comparison, same-origin-only requests, zero browser persistence, and no console or page
errors. It also injected an HTML payload into untrusted provider/component fields and confirmed
text-only rendering with no markup nodes, script execution, or external request. Its result and
screenshots are in `docs/evidence/browser-ui-20260910.json` and the two adjacent PNG files. The
run covers the normal synthetic flow; filesystem spies and native storage enforcement remain
separate evidence limits.

The companion harness branch records a bounded baseline/successor differential run for Astra and
Ollama with synthetic fake downstreams; its machine-readable result is
`docs/evidence/context-capture-fidelity-20260910.json`. That evidence covers the bridge handoff and
failure cases only. It does not establish a real provider or game receipt, integrated
producer-to-browser execution, or independent security review.
