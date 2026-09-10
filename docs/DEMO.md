# Offline demonstration

Run:

```text
cargo run --locked --package context-service --bin context-console -- demo
cargo run --locked --package context-service --bin context-console -- phase2-demo
```

The command uses only checked-in synthetic fixtures. It parses and stores one complete CLI-boundary
snapshot plus one metadata-only snapshot, appends seven allowlisted lifecycle events, issues a
short-lived in-memory read capability, and sends one authenticated GET through the in-process read
API. It prints `offline_demo=true`, `provider_calls=0`, `game_launches=0`, the two snapshot IDs,
`retained_events=7`, a successful API status, and `read_api_is_mutation_free=true`.

`phase2-demo` runs the controlled-context state machine over the synthetic scope. It creates a
draft, includes an editable history item, builds an exploratory preview, pauses, builds the
applicable preview, commits a revision while paused, and resumes explicitly. Its output includes
the prepared manifest digest, separate commit/resume effects, `provider_calls=0`, and
`game_launches=0`.

The integrated management fixture starts with an opt-in encrypted SQLite control journal, reopens
it to exercise controller-epoch recovery, and persists successful draft/preview/pause/commit/resume
mutations before publishing its live projection. Run the local migration and rollback checks with:

```text
cargo test --locked --package context-service --test phase2_durable
```

The six test results are recorded in
[`docs/evidence/phase2-durable-store-20260910.json`](evidence/phase2-durable-store-20260910.json).

For an integrated producer → capture → API → browser run, use the checked-in audit script. It starts
the Rust demo server, which performs the producer and memory-capture stages, serves `/web/`, and
adapts the browser's `/demo/snapshot`, `/demo/comparison`, and `/demo/events` requests through the
authenticated `ReadApi` before returning them to the page:

```text
FONTCONFIG_PATH=/tmp/chromium-libs/etc/fonts \
FONTCONFIG_FILE=/tmp/chromium-libs/etc/fonts/fonts.conf \
XDG_DATA_DIRS=/tmp/chromium-libs/usr/share:/usr/share \
LD_LIBRARY_PATH=/tmp/chromium-libs/usr/lib/x86_64-linux-gnu:/tmp/chromium-libs/lib/x86_64-linux-gnu:/tmp/chromium-libs/usr/lib \
PLAYWRIGHT_MODULE=/tmp/ascension-browser-audit/node_modules/playwright \
node tools/integrated_browser_audit.cjs
```

The run exits after the browser assertions and reports producer, capture, API, browser, provider,
game, and external-request counters. It uses only synthetic data and can be stopped with Ctrl-C
when the server is run separately.

To review the browser bundle, serve the repository root with a static server and open `/web/`:

```text
python3 -m http.server 8000
```

The browser first reads the bounded `offline-bundle.json` manifest, then fetches its metadata,
comparison, and JSONL event artifacts from the same origin. It displays the boundary, ordered
component facts, mapping, lifecycle timeline, qualified measurements, comparison result, and
disclosure states. It never contacts a provider or game, stores a capability in browser
persistence, or renders fixture strings as HTML.

A local Chromium run exercised the load, comparison, keyboard focus, reduced-motion, narrow-layout,
same-origin request, and browser-storage checks. The machine-readable result and sanitized
screenshots are committed as
[`docs/evidence/browser-ui-20260910.json`](evidence/browser-ui-20260910.json),
[`docs/evidence/browser-desktop-20260910.png`](evidence/browser-desktop-20260910.png), and
[`docs/evidence/browser-narrow-20260910.png`](evidence/browser-narrow-20260910.png). The record
identifies Chromium `153.0.8010.12`, both viewport sizes, zero external requests, empty browser
storage, and the keyboard comparison outcome.

The integrated result and screenshots are
[`docs/evidence/integrated-browser-ui-20260910.json`](evidence/integrated-browser-ui-20260910.json),
[`docs/evidence/integrated-browser-desktop-20260910.png`](evidence/integrated-browser-desktop-20260910.png),
and [`docs/evidence/integrated-browser-narrow-20260910.png`](evidence/integrated-browser-narrow-20260910.png).

The Phase 2 control workflow has a separate executable browser audit. Run it with the same bounded
Chromium library environment:

```text
FONTCONFIG_PATH=/tmp/chromium-libs/etc/fonts \
FONTCONFIG_FILE=/tmp/chromium-libs/etc/fonts/fonts.conf \
XDG_DATA_DIRS=/tmp/chromium-libs/usr/share:/usr/share \
LD_LIBRARY_PATH=/tmp/chromium-libs/usr/lib/x86_64-linux-gnu:/tmp/chromium-libs/lib/x86_64-linux-gnu:/tmp/chromium-libs/usr/lib \
PLAYWRIGHT_MODULE=/tmp/ascension-browser-audit/node_modules/playwright \
node tools/phase2_browser_audit.cjs
```

It exercises draft creation and save, note/objective authorization, exploratory and applicable
previews, pause readiness, commit while paused, explicit resume, control events, zero provider/game
effects, narrow layout, and browser-storage/network assertions. The result and screenshots are
[`docs/evidence/phase2-browser-ui-20260910.json`](evidence/phase2-browser-ui-20260910.json),
[`docs/evidence/phase2-browser-desktop-20260910.png`](evidence/phase2-browser-desktop-20260910.png),
and [`docs/evidence/phase2-browser-narrow-20260910.png`](evidence/phase2-browser-narrow-20260910.png).

This is source/synthetic evidence plus a local browser observation over synthetic data. It does not
claim a live provider receipt, actual game launch, production or native storage enforcement, or
cross-platform browser compatibility. The companion harness repository records fake-only
Astra/Ollama process fidelity against synthetic downstreams.
