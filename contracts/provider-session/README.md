# Provider-session contract consumer

This directory is the target-owned, additive Phase 4 client contract. The harness remains the
authority for session ownership, native operation journaling, continuity, and scheduler admission.
The `provider-session.v1` envelope is metadata-only and preserves the strict Phase 1–3 schemas.

The public API uses the `ascension.provider-session.api-result.v1` envelope. The fixture route is
opt-in (`fixture_only`) and has no provider credentials, native process, raw RPC method, or game
capability. Reads and plans report zero inference/game effects; only the harness-owned scheduler
can submit a prepared turn after the existing Phase 2 commit/resume flow.
Native history coverage is explicitly `unknown` unless a pinned adapter supplies a verified
watermark. Remote erasure is never inferred from local cleanup.
