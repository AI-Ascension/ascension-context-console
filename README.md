# Ascension Context Console

Private Phase 1 read-only inspector for application-controlled context sent through AI-Ascension
decision paths.

This checkout contains the Phase 1 source delivery: a bounded Rust snapshot/event reader, a
scoped in-memory store and loopback read API, authenticated private-content vault primitives, an
offline CLI demonstration, and a same-origin browser view. The console reports what the harness
prepared at a named boundary. It does not invoke a provider or game and cannot edit, compact,
retrieve, pause, resume, or submit context.

The companion capture seam lives in the accepted `AI-Ascension/sts2-harness` source tree. It
captures the actual Exo session and generic provider bytes, and the Astra/Ollama bridge binaries
expose the final stdin/schema/configuration or serialized-body seam. Capture is sideband-only and
fail-soft; the default mode is off.

Run the local gates and offline demonstration with the pinned Rust toolchain:

```text
cargo run --locked --package repo-policy -- --strict
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo run --locked --package context-service --bin context-console -- demo
```

The browser fixture can be served from the repository root with any static file server and opened
at `/web/`. It reads only the checked-in synthetic bundle. A local Chromium run is recorded in
[`docs/evidence/browser-ui-20260910.json`](docs/evidence/browser-ui-20260910.json) with desktop and
narrow screenshots. See [docs/DEMO.md](docs/DEMO.md), [docs/API.md](docs/API.md), and
[docs/SECURITY.md](docs/SECURITY.md) for the evidence boundary and operational limits.
