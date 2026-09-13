# ADR 0019: injected harness-owner memory and provider-session facades

Status: accepted for the repository-owned Phase 3/4 consumer boundary. The production owner
implementation and its concrete wire transport remain external to this checkout.

## Decision

Add one target-owned `HarnessOwner` trait and an explicit `HarnessOwnerComposition`. The memory and
provider-session routes can delegate only when constructed with that composition. The default
fixture/unattached constructors remain unchanged and continue to report unavailable or
fixture-only behavior.

The port carries a closed `OwnerOperation`, an exact scope, an optional bounded opaque reference,
the already-validated closed JSON body, a request digest and the grant lane. It does not carry
provider/game credentials, URLs, filesystem paths, native RPC frames, native IDs or private
capture bytes. The harness remains the authority for corpus admission/retrieval, generation,
session state, compaction, scheduler admission and game dispatch.

Three independent grant lanes are enforced at the target boundary:

- `read_search` for capability/status/query and session discovery/history;
- `generation_review` for memory generation and review; and
- `control` for memory selection and provider-session control/compaction.

Grants are exact-scope, finite, host/origin checked and CSRF checked for writes. Revocation is
shared by routes cloned from one composition. A lost owner response performs one receipt lookup and
preserves `accepted`, `unknown` or `unsupported`; the facade never retries an operation that may
have taken effect. Read/history replies reporting inference, native or game effects fail closed.

## Compatibility and evidence

The change is additive to the existing `/v3/memory` and provider-session fixture surfaces. Existing
schemas and fixture constructors remain supported. The new owner receipt schema is a target-to-owner
port contract pending the external harness interface decision; no concrete native protocol or
provider behavior is claimed here. `tests/owner_delegation.rs` is synthetic recording-owner
evidence only. Live harness, provider, native-session and game evidence remain unverified external
gates tracked by issue #19 dependencies.
