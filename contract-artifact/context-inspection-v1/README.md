# Proposed contract seeds

These Draft 2020-12 schemas and OpenAPI 3.1 document are original implementation seeds, not existing runtime interfaces or a published service. T04/root must adopt or deliberately refine the versioned format after current source/policy discovery. Required semantics in the specs and acceptance catalogue cannot be weakened by a schema change.

`context-snapshot.schema.json` describes immutable preparation capture at a named application boundary; `context-event.schema.json` adds later facts without rewriting snapshots; `token-measurement.schema.json` prohibits zero for unavailable counts; and `capabilities.schema.json` fixes management controls to false for Phase 1. The OpenAPI seed contains GET operations only and is distinct from the privileged local ingest contract.

The schemas validate shape, bounds and some conditional invariants. Semantic code must additionally validate unique component IDs and contiguous ordinals, reference integrity, scope/producer authenticity, mapping references, actual retained-byte digests, transport causality, immutable-ID conflicts, measurement scope, storage approval and retention. JSON Schema alone cannot establish exact provider handoff or content permission.

A complete CLI-boundary manifest in this seed requires stdin, output_schema and configuration components. The HTTP-boundary seed requires the actual serialized body. Raw hashes cover retained bytes, not redacted projections. A derived expired view is not a mutation to an old snapshot: retain its original manifest and add an expiration event/read-model status.

The fixture blob files are static original test inputs. Their presence on disk does not represent an actual memory-mode runtime writing private content. The demo implementation must generate valid current harness requests using its validated builders; these generic interchange fixtures alone are not necessarily valid game observations.
