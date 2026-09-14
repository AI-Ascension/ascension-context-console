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
Phase 3 target implementation `9b69951` and companion harness implementation `b923192`. The
current Phase 3 branch remains a draft and has not been merged, released, deployed, or used with a
live provider or game.

All new payloads use the `ascension.context-memory.*.v1` namespace. `context_compact=false`,
provider-session and direct-game-dispatch meanings under the older capability profile remain
unchanged. Memory defaults to disabled in the integrated target facade; enabling it requires the
harness owner and the existing Phase 2 approval serializer.

## Effective-limit migration pin (issue #24)

This surface now consumes the harness effective-limit contract
(`ascension.harness.effective-limits.v1`, ADR 0020). The capability artifact was overwritten with
the authoritative harness producer bytes; the policy artifact was already byte-identical.

Producer: `AI-Ascension/sts2-harness`, `origin/main` revision
`f8015e52ccb530e60d722283ef2b063da372169b`.

| Artifact | sha256 |
|---|---|
| `contracts/context-memory/policy.schema.json` | `55c9bba8ec71ae00b1a6c6bec07b1ca85ec9df0de0d7a0bfa2b5cd663f9aa416` |
| `contracts/context-memory/capabilities.schema.json` | `2b980bbdcdd886398c1e590303b82afee174e4569164e79b210ab35f6669bd22` |

The advertised capability schema is `ascension.context-memory.capabilities.v3`, whose descriptor
requires the `effective_limits` and `binding` objects. A dual reader keeps
`ascension.context-memory.capabilities.v1` payloads readable while Studio adopts `v3`.

Values are admitted only after the published record is authenticated against the derivation of the
validated trusted descriptor; an absent field is `field_not_advertised`, never unlimited. The
closed fail-closed reasons are `effective_limit_exceeded`, `disabled`, `field_not_advertised`,
`descriptor_stale`, `descriptor_tampered`, `profile_mismatch`, `consumer_not_recorded`, and
`consumer_pin_not_adopted`.

Evidence is synthetic/fixture/contract-level only: the producer bytes are pinned by digest and the
fixture ceilings equal the portable schema ceilings. No native, provider, owner, deployment, or
real-owner behavior is claimed.
