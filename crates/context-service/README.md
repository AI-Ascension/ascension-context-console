# context-service

Restricted local ingestion and read projections for the Context Console. The crate has no provider,
game, process-execution or gateway dependency. It accepts only bounded immutable manifests and
exposes scoped read operations; capture is default off.

## Modules

- `http/` - bounded request framing, target parsing and responses.
- `store/` - bounded in-memory snapshots, append-only events, grants, cursors and comparison.
- `read_api/` - authenticated read router, handlers and capability contract.
- `capture/` - off/metadata/memory/private capture sink contract.
- `private_store/` - approved authenticated encrypted vault primitive, held in memory.
- `telemetry/` - bounded capture accounting.
- `demo/` - provider-free offline demonstration server.
- `bin/context-console/` - the `health|demo|integrated-demo [port]|inspect [snapshot.json]` CLI.
- `bin/integrated-demo/` - additive integrated demonstration binary.

## Public surface

`lib.rs` is the single owner of the crate re-exports, including `Store`, `ReadApi`, `ReadGrant`,
`CaptureMode`, `MemoryCapture`, `PrivateVault`, `run_integrated_demo` and the bounded `MAX_*`
constants. Consumers use those re-exports rather than module paths.

## Usage

```text
cargo run --locked --package context-service --bin context-console -- demo
cargo run --locked --package context-service --bin context-console -- inspect fixtures/valid/snapshot-cli.json
cargo run --locked --package context-service --bin integrated-demo -- 0
```

The demonstrations use checked-in synthetic fixtures and loopback requests only. They are synthetic
evidence: they make no provider or game call and do not prove live capture, persistent encrypted
storage or native behavior.
