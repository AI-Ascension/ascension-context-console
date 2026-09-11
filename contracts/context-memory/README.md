# Phase 3 context-memory contract pin

These versioned JSON Schema and OpenAPI artifacts define the target-facing Phase 3 memory
namespace. They are additive: the Phase 1 inspection and Phase 2 `context-control.v1` files are
unchanged. The harness owns admission, retrieval, compaction and policy invariants; this target
consumes the contracts through the bounded facade in `src/memory.rs` and never writes the harness
store directly.

The schemas are closed and bounded. Runtime code additionally checks scope, source digests,
causal cutoffs, revocation epochs, source spans, review bindings, whole-input digests and the
`local_read_no_inference`/`authorized_summary_generation_only` effect classes. The OpenAPI file is
an operator contract only; local route tests are the executable target evidence.

The artifacts were copied from the Phase 3 package at implementation time and are pinned to the
Phase 2 target head `0c1f402b0b6c7f0ab79eb369a649286a46482e3a` and companion harness head
`8674874feccfbf995ed0aa5a8ec8390d9dac137b`. The current Phase 3 branch remains a draft and has
not been merged, released, deployed, or used with a live provider or game.

All new payloads use the `ascension.context-memory.*.v1` namespace. `context_compact=false`,
provider-session and direct-game-dispatch meanings under the older capability profile remain
unchanged. Memory defaults to disabled in the integrated target facade; enabling it requires the
harness owner and the existing Phase 2 approval serializer.
