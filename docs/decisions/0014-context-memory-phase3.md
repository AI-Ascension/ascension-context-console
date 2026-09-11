# ADR 0014: bounded context memory facade

Status: accepted for the Phase 3 draft branch. Owner: context-console target and harness
maintainers jointly at their existing boundaries.

Phase 3 adds the `ascension.context-memory.*.v1` namespace beside the frozen Phase 1 inspection
and Phase 2 control namespaces. The harness owns source admission, causal membership, lexical
ranking, exact extraction, summary-job admission/review, selection manifests and revocation. The
target owns only the authenticated `/v3/memory` facade and operator presentation. The facade does
not open a corpus database, resolve a client path, invoke a provider, launch a process, or dispatch
a game action.

Memory is disabled by default. Search is a local-read effect with a bounded body and no inference;
generation requires a distinct review-capable principal and remains unsupported by the target
fixture until a harness adapter is explicitly attached. All generated proposals remain
review-required and `applied=false`. Selection records the whole rendered byte digest, mandatory
protected bytes, optional budget and Phase 2 prepared-manifest identity. Commit is held and an
explicit existing Phase 2 resume is the only release operation.

The initial lexical implementation uses Unicode alphanumeric lower-casing, a frozen small
stop-word set, occurrence-count relevance scores and `(score desc, observed sequence asc, id asc)`
ties. Scores are relevance signals, never confidence. Corpus generations and revocation epochs are
separate; future, late, sibling, private, expired and revoked records are filtered before ranking.
Indices remain disposable projections.

The rejected alternative was changing the old capability's `context_compact` field or putting a
second memory store in the console. Both would let an older consumer silently reinterpret the new
authority and would duplicate the harness owner. The additive profile preserves legacy bytes and
keeps unsupported vector retrieval, persistent provider sessions, hidden reasoning and direct game
dispatch explicit.
