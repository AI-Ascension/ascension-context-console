# Architecture

The harness owns request construction, capture records, provider boundaries, and decision lineage.
The console owns validation, bounded retention, scoped read projections, and presentation. There
is no provider, game, gateway, MCP, credential, arbitrary URL, or arbitrary process dependency in
this repository.

The reader crate is split into `json/` for the bounded parser and accessors, `snapshot/` for
snapshot types, parsing, mapping and measurement, and `event/` for lifecycle-event types and
parsing, with the CLI in `src/bin/context-reader/`. The service crate is split into `http/`,
`store/`, `read_api/`, `capture/`, `private_store/`, `telemetry/` and `demo/`, with the
`context-console` and `integrated-demo` binaries in `src/bin/`.

`context-reader` parses immutable snapshot and lifecycle-event JSON with a small bounded parser. It
rejects duplicate or unknown fields, invalid identity reuse, false completeness, unsupported
measurements, and forbidden raw reasoning fields. The projection retains metadata only; content is
addressed by opaque references and is never read from a path supplied by a caller.

`context-service` accepts validated snapshots and append-only events into bounded in-memory maps.
Snapshot identity reuse is idempotent only for identical bytes. Event identity reuse follows the
same rule. Event pages expose immutable append ordinals scoped to the authorized project/run, so
producer-global sequence values cannot reveal hidden positions or invalidate reconnect cursors when
late events arrive. Producer capture gaps remain explicit `capture.gap` events. `ReadGrant` scopes a
bearer capability to a project and optional run, enforces a finite expiry, and supports revocation.
`ReadApi` permits only GET, exact Host/Origin values, and loopback binding; every response is
`no-store`. The browser starts from the bounded same-origin `offline-bundle.json` manifest and
rejects absolute, escaped, encoded, or cross-origin artifact paths before fetching them.

Capture modes have separate contracts. Off mode uses a no-op sink and does not hash or copy input.
Metadata mode retains bounded facts without content or digests. Memory mode retains bounded bytes
in a process ring. Private mode requires policy approval and restricted authorization before
XChaCha20-Poly1305 authenticated encryption with project/snapshot/component/content-reference-bound associated data; unsafe
setup fails closed. The plaintext store rejects private-mode content references rather than serving
them as plaintext. The browser does not use
localStorage, IndexedDB, cache storage, external resources, or credential-bearing URLs.

The checked-in fixtures and `context-console demo` exercise producer -> store -> read API evidence
without launching a provider or game. The `context-console integrated-demo` subcommand and the
additive `integrated-demo` binary extend that synthetic path through `MemoryCapture`, an
authenticated `ReadApi` projection, and the browser's same-origin `/demo/*` requests; its metrics
expose every stage and the zero provider/game counters. The browser
evidence also loads the checked-in synthetic bundle from a loopback static server and records the
rendered projection, comparison, keyboard flow, and browser-storage state. The companion harness branch supplies the actual Exo,
generic-provider, Astra, and Ollama capture seams. Its bounded fake-process bridge oracle uses
production source `316c8bd1814d9f9762a08c534898ec827365c91a` and is recorded on the companion
evidence branch tracked by PR #51; live provider receipts, game launches, integrated
producer-to-browser execution through the actual harness, and native storage behavior remain
unverified.
