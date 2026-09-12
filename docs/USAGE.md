# Local review

Use Rust 1.97.1 from `rust-toolchain.toml`. The reader accepts a bounded snapshot path or stdin:

```text
cargo run --locked --package context-reader -- fixtures/synthetic/snapshot.json
cat fixtures/synthetic/snapshot.json | cargo run --locked --package context-reader --
```

The service CLI provides a provider-free integrated demonstration and a bounded inspection command:

```text
cargo run --locked --package context-service --bin context-console -- demo
cargo run --locked --package context-service --bin context-console -- phase2-demo
cargo run --locked --package context-service --bin context-console -- inspect fixtures/valid/snapshot-cli.json
```

The compiled Phase 2 operator CLI uses a bounded encrypted SQLite fixture and the same typed
control reducer as the HTTP workflow:

```text
STORE=/tmp/context-control.sqlite
cargo run --locked --package context-service --bin context-console -- phase2-cli init "$STORE"
cargo run --locked --package context-service --bin context-console -- phase2-cli capabilities "$STORE"
cargo run --locked --package context-service --bin context-console -- phase2-cli eligible "$STORE"
cargo run --locked --package context-service --bin context-console -- phase2-cli draft-create "$STORE"
```

Create a patch JSON file only when needed, and pipe it to `draft-edit` so note text does not enter
shell history or process arguments. The command supports typed include/exclude/pin/unpin, notes,
objective authorization, and restore operations. Then use `preview`, `pause`, `commit`, and
`resume` with the returned draft/preview IDs; `draft-show`, `preview-show`, `revisions`, `events`,
and `command` resolve authoritative state after a lost response. `eligible` returns metadata and a
null `content` field by default; explicit content output requires the fixture content capability
environment variable. Objective edits require the separate fixture objective capability variable.

The CLI output schema is `ascension.context-control.cli-result.v1`. Exit codes are 0 for completed
or accepted commands, 2 for usage/validation, 3 for stale/conflict, 4 for denied, and 5 when the
durable store is unavailable. The fixture key and capability tokens are intentionally local test
values; this command is provider-free, game-free, and not a production authentication boundary.

`demo` ingests the CLI and metadata fixtures, appends the seven synthetic lifecycle events, issues
an in-memory content grant, and makes one authenticated read-only API request. Its output includes
`provider_calls=0`, `game_launches=0`, the snapshot identities, event count, and mutation-free API
status.

`phase2-demo` is the bounded control proof. It uses the same typed control plane as the integrated
API to create and edit a draft, distinguish exploratory from applicable preview, pause, commit
while held, and resume explicitly. It never calls a provider or game. For HTTP review, run
`integrated-demo 0` and use the `/v2/runs/fixture-run/context-control/` routes with the fixture
editor capability and exact loopback Origin/CSRF headers.

The browser editor exposes the same typed operations: editable rows can be included, excluded,
and pinned or unpinned; a retained revision can be restored into the current draft; and an
attributed browser note can be removed with its expected version. Restore and removal invalidate
any prior preview and never change a host snapshot or game state.

For the integrated synthetic browser path, run `tools/browser-audit/integrated_browser_audit.cjs` with the
Playwright module and Chromium library environment shown in [DEMO.md](DEMO.md). It starts the
loopback `integrated-demo` server and verifies that the browser receives API-derived projections
after producer and memory-capture stages.

For a browser-only review, serve the repository root so the page can read its same-origin fixture:

```text
python3 -m http.server 8000
```

Open `http://127.0.0.1:8000/web/`. Do not open the page from a `file:` URL; browsers correctly
restrict cross-file fetches. The page reads the same-origin `offline-bundle.json` manifest and its
declared artifacts, then displays metadata, ordered components and adapter mappings. It does not
contact a provider or a game.

The checked-in browser evidence was captured with Chromium `153.0.8010.12` at desktop and narrow
viewports. See [`docs/evidence/browser-ui-20260910.json`](evidence/browser-ui-20260910.json) and
the adjacent screenshots for the exact request, storage, keyboard, and comparison outcomes.

The additive journal recovery and safe deactivation procedure is in
[`docs/MIGRATION.md`](MIGRATION.md). It preserves Phase 1 snapshots and keeps a deactivated
management fixture read-only. The target's encrypted SQLite migration, rollback, backup, and
legacy-active refusal checks are run with `cargo test --locked --package context-service --test
phase2_durable` and recorded in
[`docs/evidence/phase2-durable-store-20260910.json`](evidence/phase2-durable-store-20260910.json).

The browser bundle is synthetic evidence. A successful parse, demo, or render does not prove a live
capture, provider receipt, production persistent storage, native-platform behavior, or target-game
compatibility. The durable-store record is separate local component evidence. Do not put credentials
or raw private captures in fixtures.
