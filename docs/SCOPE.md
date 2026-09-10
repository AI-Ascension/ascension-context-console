# Phase 1 scope

This delivery implements a truthful read-only Context Console. The Rust reader accepts bounded
`ascension.context-snapshot.v1` manifests and `ascension.context-event.v1` lifecycle records,
validates their semantic invariants, and returns immutable projections with component, mapping,
usage, and lineage summaries. It keeps no raw JSON in the reader projection and never follows a
caller-provided filesystem path for content.

The browser page in `web/` reads the checked-in synthetic metadata, comparison, and event fixtures
with same-origin, cache-disabled fetches. It uses `textContent` for all untrusted values and renders
ordered components, mapping, timeline, measurements, comparison, and disclosure states. The page
does not write browser persistence or make provider/game calls.

The supported capture vocabulary is explicit:

| Mode | Bootstrap meaning |
| --- | --- |
| `off` | No capture work is started. The reader has no source to display. |
| `metadata` | Bounded IDs, sizes, statuses and mappings; prompt bytes are unavailable. |
| `memory` | Opt-in bounded content held by an application-owned memory ring; no disk fallback. |
| `private` | Opt-in classified content in authenticated encrypted, access-restricted vault storage after policy approval. |

The included fixtures cover memory and metadata projections. The service tests cover memory bounds
and private authenticated encryption; the private vault itself is an in-memory primitive and does
not claim a persistent filesystem writer.

The console cannot submit a prompt, invoke a provider, mutate a game, edit a context, pause a run,
or read arbitrary files or URLs. Provider-added context, hidden instructions and hidden reasoning
remain `not_exposed`. The accepted harness commit and synthetic fixture provenance are recorded in
the fixtures and `docs/PROVENANCE.md`. Native platform, live provider, and target-game evidence
remains separate.
