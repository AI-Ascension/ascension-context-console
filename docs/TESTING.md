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
including prepared/write-failed states; bridge unit tests cover the final Astra and Ollama input
component seams.

The checked-in browser and CLI demo are offline synthetic evidence. Actual-process fake downstream
oracles, differential pre/post behavior, native filesystem/process checks, and independent security
review are separate evidence gates. They must not be inferred from these local tests.
