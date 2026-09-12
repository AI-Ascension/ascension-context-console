# ADR 0001: Read-only inspection boundary

## Status

Accepted. This record extracts the boundary already fixed by `AGENTS.md`,
`docs/ARCHITECTURE.md`, and `docs/SCOPE.md`. It adds no new claims and does not upgrade any
evidence label.

## Context

The companion harness owns request assembly, capture records, provider boundaries, and decision
lineage. The Context Console owns bounded validation, restricted ingest and storage, scoped read
projections, the read API/CLI, and browser presentation. Keeping inspection as a separate,
non-mutating concern is what prevents the tool from influencing a provider request or game state.

## Decision

Each statement below carries the evidence label for the restatement itself. `source-derived` means
the statement is taken from the accepted sources named in Status; it is not an independent
measurement.

1. **Read-only inspection** (`source-derived`). The console reports the bounded component,
   mapping, usage and lineage projections of application-controlled snapshot and event records at a
   named boundary.
2. **No mutation or control** (`source-derived`). The console cannot submit a prompt, invoke a
   provider, mutate a game, edit a context, apply, compact, retrieve, pause or resume a run, start
   persistent sessions, merge a map, or make provider calls.
3. **No ambient authority** (`source-derived`). The repository has no provider, game, gateway, MCP,
   credential, arbitrary URL, or arbitrary process dependency, and it reads no arbitrary files or
   URLs. The reader never follows a caller-provided filesystem path for content; content is
   addressed by opaque references.
4. **Capture is default off** (`source-derived`). Capture modes have separate contracts: `off` does
   no capture work; `metadata` retains bounded facts without content or digests; `memory` retains
   bounded bytes in an application-owned process ring with no disk fallback; `private` requires an
   accepted policy exception and authenticated encryption, and unsafe setup fails closed.
5. **Bounded local read surface** (`source-derived`). Read grants are scoped to a project and
   optional run, expire, and can be revoked. The read API permits only GET, exact Host and Origin
   values, and loopback binding, and every response is `no-store`. Snapshot identity reuse is
   idempotent only for identical bytes, and lifecycle pages are scoped and append-ordered.
6. **Presentation boundary** (`source-derived`). The browser reads the checked-in synthetic
   artifacts named by the bounded `offline-bundle.json` manifest with same-origin, cache-disabled
   fetches. It renders untrusted values as text, writes no browser persistence, makes no
   provider/game calls, and rejects absolute, escaped, encoded, or cross-origin artifact paths
   before fetching them.
7. **Disclosure limit** (`source-derived`). Provider-added context, hidden instructions, and hidden
   reasoning remain `not_exposed`. Synthetic fixtures prove only local behavior.

## Evidence limits

- `unverified`: live provider receipts, game launches, integrated producer-to-browser execution
  through the actual companion harness, native storage behavior, and cross-platform browser
  compatibility.
- `unsupported`: map/image capture until an accepted producer boundary supplies it, and provider,
  game-host, Windows, deployment, and external observability compatibility claims.
- `proposed`: a future persistent collector TTL, active-reference, and tombstone implementation;
  the private vault in this delivery is an in-memory primitive.
- `inferred`: none. This record deliberately avoids extending the accepted sources.

## Consequences

The boundary is enforced by rejection rather than by documentation alone: forbidden fields, caller
paths, non-loopback binds, mismatched scopes, expired grants, and unsafe private setup are refused.
Any future work that would cross this boundary requires a new accepted decision.
