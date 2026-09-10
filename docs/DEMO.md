# Offline demonstration

Run:

```text
cargo run --locked --package context-service --bin context-console -- demo
```

The command uses only checked-in synthetic fixtures. It parses and stores one complete CLI-boundary
snapshot plus one metadata-only snapshot, appends seven allowlisted lifecycle events, issues a
short-lived in-memory read capability, and sends one authenticated GET through the in-process read
API. It prints `offline_demo=true`, `provider_calls=0`, `game_launches=0`, the two snapshot IDs,
`retained_events=7`, a successful API status, and `read_api_is_mutation_free=true`.

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

This is source/synthetic evidence plus a local browser observation over synthetic data. It does not
claim a live provider receipt, actual game launch, native storage enforcement, or cross-platform
browser compatibility. The companion harness repository separately records fake-only Astra/Ollama
process fidelity against synthetic downstreams.
