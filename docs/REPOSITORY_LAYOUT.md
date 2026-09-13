# Repository layout

`crates/context-reader` contains the pure bounded manifest validation and projections, split into
`json/` (bounded parser and accessors), `snapshot/` (snapshot types, parsing, mapping and
measurement), and `event/` (lifecycle event types and parsing). Its CLI entry point is
`src/bin/context-reader/`, and its crate-level integration tests live in `crates/context-reader/tests/`.

`crates/context-service` contains the local store, read authorization, HTTP boundary, capture seam,
private vault and the CLI/API/demo boundary:

- `http/` - request framing, target parsing and bounded responses.
- `store/` - bounded in-memory snapshots, events, grants, cursors and comparison.
- `read_api/` - router, handlers, capabilities and authenticated reads.
- `capture/` - the off/metadata/memory/private capture sink contract.
- `private_store/` - the approved, authenticated encrypted vault.
- `telemetry/` - bounded capture accounting.
- `harness_facade.rs` - non-demo typed harness-owner port/client, scoped grants, same-origin
  transport adapter, redacted projections, and stable owner outcomes.
- `demo/` - the provider-free offline demonstration server.
- `src/bin/context-console/` - the `health|demo|integrated-demo|inspect` CLI.
- `src/bin/integrated-demo/` - the additive integrated demonstration binary.

Its Phase 1 crate-level integration tests live in `crates/context-service/tests/`. The Phase 2-4
integration tests remain in the repository-root `tests/` directory and are wired as explicit
`[[test]]` targets in `crates/context-service/Cargo.toml`.

`contract-artifact/context-inspection-v1` is the pinned schema copy. `fixtures/` contains original
synthetic records. `web/` is a small static read-only frontend: `index.html`, `css/styles.css`, and
the `js/` modules (`app.js`, `api.js`, `bundle.js`, `render.js`). `tools/repo-policy` provides the
local policy gate, and `tools/browser-audit` drives the provider-free integrated
producer/capture/API/browser demonstration. `docs/` holds the architecture, scope, policy and
evidence records, including the `docs/decisions/` records.
