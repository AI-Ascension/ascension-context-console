# Offline demonstration

Run:

```text
cargo run --locked --package context-service --bin context-console -- demo
cargo run --locked --package context-service --bin context-console -- phase2-demo
cargo run --locked --package context-service --bin context-console -- phase3-cli capabilities
cargo run --locked --package context-service --bin context-console -- phase3-cli status
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

The Phase 3 CLI is a bounded inspection path over the additive memory facade. Because this target
checkout has no attached harness projection, its capability result is disabled and its status
reports unavailable generation values; the harness CLI is the enabled synthetic policy lane:

```text
cargo run --locked --package context-service --bin context-console -- phase3-cli capabilities
cargo run --locked --package context-service --bin context-console -- phase3-cli status
printf '%s' '{"schema":"ascension.context-memory.query.v1","scope":{"project_id":"project-fixture","run_id":"run-fixture","episode_id":"episode-fixture","agent_id":"agent-fixture"},"branch_id":"branch-a","query":"settled","cutoff":10,"corpus_generation":10,"ranker_version":"lexical-v1","limit":8,"max_candidates":64,"effect_class":"local_read_no_inference"}' \
  | cargo run --locked --package context-service --bin context-console -- phase3-cli search
```

The integrated fixture starts with memory disabled, so the search result is explicitly
`projection_unavailable`, contains no candidates, and reports zero inference calls. The harness
tests exercise the enabled synthetic corpus for scope/cutoff filtering, deterministic lexical
ranking, exact extracts, isolated fake summary jobs, review/admission, whole-input selection,
approval held state, and revocation. They do not claim a live provider, native game, deployment,
or three-level native development hierarchy.

The equivalent compiled CLI can run the same operations against its opt-in encrypted fixture:

```text
STORE=/tmp/context-control.sqlite
cargo run --locked --package context-service --bin context-console -- phase2-cli init "$STORE"
cargo run --locked --package context-service --bin context-console -- phase2-cli draft-create "$STORE"
cargo run --locked --package context-service --bin context-console -- phase2-cli pause "$STORE" pause-demo
cargo run --locked --package context-service --bin context-console -- phase2-cli state "$STORE"
```

`phase2-cli draft-edit` consumes a bounded typed patch from stdin, `eligible` redacts content by
default, and the preview/commit/resume commands retain the same idempotency and boundary checks as
the HTTP fixture. Its machine output is metadata-first JSON and every successful operation is
persisted before output. Reopening an operator command preserves the durable operator epoch; an
actual controller recovery uses the recovery API and fences continuation previews. No CLI command
contacts a provider, game, network URL, or external process.

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
