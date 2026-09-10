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

`demo` ingests the CLI and metadata fixtures, appends the seven synthetic lifecycle events, issues
an in-memory content grant, and makes one authenticated read-only API request. Its output includes
`provider_calls=0`, `game_launches=0`, the snapshot identities, event count, and mutation-free API
status.

`phase2-demo` is the bounded control proof. It uses the same typed control plane as the integrated
API to create and edit a draft, distinguish exploratory from applicable preview, pause, commit
while held, and resume explicitly. It never calls a provider or game. For HTTP review, run
`integrated-demo 0` and use the `/v2/runs/fixture-run/context-control/` routes with the fixture
editor capability and exact loopback Origin/CSRF headers.

For the integrated synthetic browser path, run `tools/integrated_browser_audit.cjs` with the
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

The browser bundle is synthetic evidence. A successful parse, demo, or render does not prove a live
capture, provider receipt, persistent encrypted storage, native-platform behavior, or target-game
compatibility. Do not put credentials or raw private captures in fixtures.
