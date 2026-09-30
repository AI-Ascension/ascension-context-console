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

## Effective-limit migration pin (issues #24, #780)

This surface now consumes the harness effective-limit contract
(`ascension.harness.effective-limits.v1`, ADR 0020). The capabilities and policy artifacts were
overwritten with the authoritative harness producer bytes. The policy artifact tightens `version`
and `epoch` to `minimum: 1` under the unchanged `$id`; the capabilities artifact widens
`hardening.encrypted_state` from `const true` to `boolean` and admits slash-containing
`enabled_methods` names.

Producer: `AI-Ascension/sts2-harness`, `origin/main` revision
`f8015e52ccb530e60d722283ef2b063da372169b` for the `v3` artifacts, and `27f1d4219e98999df4903615a57236e4747ae932`
(`Refs #755`) for the `v4` artifacts below.

| Artifact | sha256 |
|---|---|
| `contracts/provider-session/policy.schema.json` | `85d5f36900e10fa1e60c918e6c1fc4fbdbd9432093a65738b2098de142136234` |
| `contracts/provider-session/capabilities.schema.json` | `accec38f7a6bb58fd485bf74a8a2ed7aca354fd80b2eff3dae8363a30dae96c9` |

The coordinated cutover serves `ascension.provider-session.capabilities.v4` with validated payload
digests, `effective_limits` and `binding`. Attached presentation requires an actual producer
record sidecar and independent owner/scope/profile configuration. The v1, v3 and v4 readers all
remain; explicit `CapabilityVersion::V1` rollback omits executable limits and reports a pending
consumer pin. See [ADR 0021](../../docs/decisions/0021-owner-capability-sidecar.md).

### `v4` qualifier rename (`evidence` -> `provenance`)

The producer renamed the descriptor's qualifier field `evidence` to `provenance` and moved the
`binding.owner_revision` const to `harness-provider-session-v4`. The rename is wire-visible: both
schemas set `additionalProperties: false` and require the new name, so a `v3` payload is not
silently accepted as `v4`. `provenance` records *how a build was qualified*, not an admission
control; `validate_descriptor()` does not read it beyond the enum check and every value is equally
admissible. The policy schema gained only a `description`; its `$id` is still
`urn:ascension:provider-session:policy:v1` and its constraints are unchanged, so the policy digest
moved purely because of that prose.

Because the console is mid-cutover, a still-valid `v3` owner reply is admitted through the
documented one-way `v3` -> `v4` lift (`SessionCapabilitiesV3::to_v4`), which renames the qualifier,
moves the pinned `owner_revision` const and recomputes the descriptor digest. It does **not**
rewrite `binding.policy_schema_sha256`: a genuine `v4` descriptor carries the `v4` policy digest,
so a `v3` descriptor is correctly refused rather than having a producer-owned digest re-pinned.

The pinned golden fixture `fixtures/effective-limits/producer.json` is still emitted by the `v3`
producer revision `f8015e52` and therefore still carries `v3` bytes. Regenerating it at a `v4`
producer revision requires repinning the `sts2-harness` Git dependency of
`tools/effective-limit-fixtures`, which this repository's AGENTS.md reserves to root.

The `encrypted_state` widening does **not** relax this repository's private-retention guard:
private retention still requires an accepted policy exception and authenticated encryption
(`PrivateVault::new` rejects `PolicyApproval { accepted: false, .. }`), independent of the
advertised boolean.

Values are admitted only after the published record is authenticated against the derivation of the
validated trusted descriptor; an absent field is `field_not_advertised`, never unlimited. The
closed fail-closed reasons are `effective_limit_exceeded`, `disabled`, `field_not_advertised`,
`descriptor_stale`, `descriptor_tampered`, `profile_mismatch`, `consumer_not_recorded`, and
`consumer_pin_not_adopted`.

Producer-library conformance now covers default and restricted session descriptors, preserving
fixed schema maxima independently of selected executable limits. A deliberately empty-method
synthetic vector exercises disabled record admission; the producer rejects it for native
attachment. See [original vectors and regeneration](../../fixtures/effective-limits/README.md).
The local default consumer disclosure is v3/aligned; rollback remains pending/v1. The authoritative
harness matrix still needs its separate owner update. No native, provider, owner, deployment, or
real-owner behavior is claimed.
