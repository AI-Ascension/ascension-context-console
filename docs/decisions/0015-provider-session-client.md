# ADR 0015: additive provider-session client boundary

Status: accepted for the Phase 4 fixture branch

The target exposes a typed, authenticated client projection under the provider-session namespace.
The harness remains the sole owner of native process handles, session epochs, operation intent,
continuity, source revocation, and the Phase 2 scheduler. The target never accepts a native thread
ID, arbitrary RPC method/params, provider URL, executable, credential, or game action.

The fixture route is explicitly `fixture_only`: candidate and maintenance commands return durable
operation metadata with zero provider inference and zero game effects. History reports the actual
coverage (`unknown` until verified) and inherited provider context is never labeled exact. A turn
can only be submitted by the harness after an approved Phase 2 commit and explicit resume.

The contract is additive to the frozen Phase 1/2/3 envelopes. Disabled mode leaves those routes,
bytes, retries, and action lineage unchanged. Local cleanup may close an owned binding but does not
claim provider-side erasure.

The fixture/source profile adopts tighter finite-retention limits than the proposed contract maxima:
scope expiry is capped at 24 hours, completed turns at 128, evaluation candidates at four, and the
combined fork/compaction maintenance budget at two. History refresh, turn completion and metadata
restore enforce these limits together with the aggregate byte bounds. The owned native transport
also rescans its broker-approved state tree before each allowlisted request and after each response;
an overage fences and terminates only the owned child. These are fixture/source guarantees and do
not certify encrypted native persistence or installed-binary behavior.
