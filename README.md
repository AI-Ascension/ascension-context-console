# Ascension Context Console

Bounded Context Console inspector and controlled context editor for AI-Ascension decision paths.

This checkout contains the Phase 1 snapshot/event reader and its Phase 2 extension. Phase 2 adds
scoped drafts, immutable provider-specific previews, an opt-in encrypted durable journal with recovery, and explicit
pause/commit/resume control. The console still invokes no provider or game and never edits host
state or submits a game action; the harness owns those authorities.

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
cargo test --locked --package context-service --test phase2_durable
cargo run --locked --package context-service --bin context-console -- demo
cargo run --locked --package context-service --bin context-console -- phase2-demo
```

The browser fixture can be served from the repository root with any static file server and opened
at `/web/`. It reads only the checked-in synthetic bundle. A local Chromium run is recorded in
[`docs/evidence/browser-ui-20260910.json`](docs/evidence/browser-ui-20260910.json) with desktop and
narrow screenshots. The Phase 2 control workflow audit and its sanitized screenshots are recorded
in `docs/evidence/phase2-browser-ui-20260910.json`. See [docs/DEMO.md](docs/DEMO.md),
[docs/API.md](docs/API.md), and [docs/SECURITY.md](docs/SECURITY.md) for the evidence boundary and
operational limits.

The integrated synthetic review is available through `context-console integrated-demo 0`. It
serves the browser and routes its declared `/demo/*` artifacts through the authenticated read API
after producer and memory-capture stages. The browser audit script records process counters and
terminates the local server after the run.

The Phase 2 control API is served under /v2/runs/:run_id/context-control/. The integrated fixture
reopens an opt-in encrypted SQLite control journal before serving and persists successful mutations
transactionally; the pure `phase2-demo` remains in memory. The fixture uses
separate editor and objective capabilities, an exact loopback Origin/CSRF check, bounded strict
JSON, draft CAS versions, immutable previews, and idempotent command receipts. phase2-demo
exercises the same control plane without a provider call, game launch, or external request.
Contract schemas and their source pin are in contracts/context-control.

The compiled fixture CLI uses that same typed reducer and encrypted journal:

```text
cargo run --locked --package context-service --bin context-console -- phase2-cli init /tmp/context-control.sqlite
cargo run --locked --package context-service --bin context-console -- phase2-cli state /tmp/context-control.sqlite
cargo run --locked --package context-service --bin context-console -- phase2-cli draft-create /tmp/context-control.sqlite
```

Use `phase2-cli --help` for the complete state, eligible, draft, preview, pause, commit, resume,
revision, and command-status surface. `draft-edit` reads a typed patch from stdin so note text is
not placed in process arguments. Output is JSON with schema
`ascension.context-control.cli-result.v1`; eligible content is redacted unless
`CONTEXT_CONSOLE_CONTENT_TOKEN=fixture-content-token` is explicitly set, and objective operations
require `CONTEXT_CONSOLE_OBJECTIVE_TOKEN=fixture-objective-token`. The CLI is a bounded synthetic
fixture: it makes no provider/game calls, uses a fixed local key, and does not claim production
authentication or storage. Standalone commands use the persisted operator epoch; controller-owned
recovery still increments the epoch and invalidates old continuation previews.
