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

## Effective-limit migration pin (issue #24)

This surface now consumes the harness effective-limit contract
(`ascension.harness.effective-limits.v1`, ADR 0020). The capabilities and policy artifacts were
overwritten with the authoritative harness producer bytes. The policy artifact tightens `version`
and `epoch` to `minimum: 1` under the unchanged `$id`; the capabilities artifact widens
`hardening.encrypted_state` from `const true` to `boolean` and admits slash-containing
`enabled_methods` names.

Producer: `AI-Ascension/sts2-harness`, `origin/main` revision
`f8015e52ccb530e60d722283ef2b063da372169b`.

| Artifact | sha256 |
|---|---|
| `contracts/provider-session/policy.schema.json` | `48d6dc1c75504983c3d5e6a1152c0447874eeb512e45b276ab440962e52781a5` |
| `contracts/provider-session/capabilities.schema.json` | `de1348ec7434703722b00a2ddb4bcef7cec02b3788ff14af018cf7b7efaf21f0` |

The live/served advertisement **remains** `ascension.provider-session.capabilities.v1` (ADR 0020
decision 4). The `v3` artifacts and the dual reader are present as inert preparation but are not
served until Studio adopts `v3` in the coordinated cutover. The `v3` target descriptor (with the
required `effective_limits` and `binding` objects) is reachable only through the non-served
`target_capabilities_v3()` accessor; `read_advertised_session_capabilities` reads both `v1` and
`v3` payloads.

The `encrypted_state` widening does **not** relax this repository's private-retention guard:
private retention still requires an accepted policy exception and authenticated encryption
(`PrivateVault::new` rejects `PolicyApproval { accepted: false, .. }`), independent of the
advertised boolean.

Values are admitted only after the published record is authenticated against the derivation of the
validated trusted descriptor; an absent field is `field_not_advertised`, never unlimited. The
closed fail-closed reasons are `effective_limit_exceeded`, `disabled`, `field_not_advertised`,
`descriptor_stale`, `descriptor_tampered`, `profile_mismatch`, `consumer_not_recorded`, and
`consumer_pin_not_adopted`.

Evidence is synthetic/fixture/contract-level only: the producer bytes are pinned by digest and the
fixture ceilings equal the portable schema ceilings. No native, provider, owner, deployment, or
real-owner behavior is claimed.
