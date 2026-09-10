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

To review the actual browser bundle, serve the repository root with a static server and open
`/web/`:

```text
python3 -m http.server 8000
```

The browser fetches the metadata fixture, comparison fixture, and JSONL event fixture from the same
origin. It displays the boundary, ordered component facts, mapping, lifecycle timeline, qualified
measurements, comparison result, and disclosure states. It never contacts a provider or game,
stores a capability in browser persistence, or renders fixture strings as HTML.

This is source/synthetic evidence. It does not claim a live provider receipt, actual game launch,
native storage enforcement, or browser execution. The companion harness repository separately
records fake-only Astra/Ollama process fidelity against synthetic downstreams.
