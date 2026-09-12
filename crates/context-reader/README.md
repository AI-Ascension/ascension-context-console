# context-reader

Bounded, read-only validation and projections for Context Console snapshot manifests and lifecycle
events. The library performs no I/O of its own: it validates the bytes it is given, never follows a
caller-provided filesystem path for content, and keeps content addressed by opaque references.

## Modules

- `json/` - bounded JSON parser and accessors shared by snapshot and event parsing.
- `snapshot/` - `ascension.context-snapshot.v1` types, parsing, mapping and measurement.
- `event/` - `ascension.context-event.v1` types and parsing.
- `bin/context-reader/` - CLI that prints a bounded projection for a snapshot path or stdin.

## Public surface

`lib.rs` is the single owner of the crate re-exports: `Snapshot`, `SnapshotProjection`,
`SnapshotError`, `parse_snapshot`, `MAX_SNAPSHOT_BYTES`, `CaptureMode`, `Component`,
`ComponentStatus`, `Identity`, `Mapping`, `Measurement`, `Producer`, `CaptureEvent`,
`EventDetails`, `EventError`, `EventType`, `parse_event`, and `parse_event_lines`.

## Usage

```text
cargo run --locked --package context-reader -- fixtures/synthetic/snapshot.json
cat fixtures/synthetic/snapshot.json | cargo run --locked --package context-reader --
```

Synthetic fixtures are local source evidence only; they do not prove provider, game or native
behavior. See `docs/ARCHITECTURE.md` and `docs/SECURITY.md`.
