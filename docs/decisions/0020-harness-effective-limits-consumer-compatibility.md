# ADR 0020: Harness effective-limit consumer compatibility (Context Console)

## Status

Accepted (coordinating-agent decision, 2026-09-14). The coordinated owner/consumer agreement is
recorded on `ascension-context-console#24` and `sts2-harness#95`; the schema migration is the next
PR. This record previously stood as Proposed; the four open questions are resolved below. Nothing
here authorizes a deployment or a native/provider run.

## Context

`sts2-harness` published the `ascension.harness.effective-limits.v1` classification record and a
producer/consumer pin-and-digest matrix (`contracts/effective-limits-pins.json`, harness PR #165).
The matrix records this repository as a **pending** `copied_contracts` consumer that advertises
`ascension.context-memory.capabilities.v1` / `ascension.provider-session.capabilities.v1` with
`effective_limits_advertised: false`. Studio is a pending `schema_adapter` consumer. Issue
`#24` tracks the obligation.

### Verified drift (console `main` @ `10122f3`, harness `main` @ `0320c05`)

| Artifact | Console copied digest | Harness producer digest | Status |
|---|---|---|---|
| context-memory policy | `55c9bba8…` | `55c9bba8…` | already aligned |
| context-memory capabilities | `c5558578…` (`…v1`) | `2b980bbd…` (`…v3`) | **drift** |
| provider-session policy | `678ce517…` (`…v1`) | `48d6dc1c…` (`…v1`) | **drift (tightened)** |
| provider-session capabilities | `74504053…` (`…v1`) | `de1348ec…` (`…v3`) | **drift** |

### Structural delta (computed from the producer bytes)

**Capabilities, both surfaces (`v1` → `v3`)**

- Additive and now **required**: `effective_limits` (an object of executable ceilings whose
  `policy_schema` const is the matching `…policy.v1`) and `binding` (`owner` const `sts2-harness`,
  `owner_revision` const `harness-context-memory-v3` / `harness-provider-session-v3`,
  `policy_schema_sha256`, `model_revision`, `adapter_revision`, `adapter_revision_sha256`,
  `descriptor_sha256`).
- `schema.const` / `$id` version bump `…:capabilities:v1` → `…:capabilities:v3`. No property is
  removed.
- provider-session: `hardening.encrypted_state` changes from `const: true` to `type: boolean`
  (a widening).
- provider-session: `enabled_methods.items.pattern` widens from `^[A-Za-z0-9][A-Za-z0-9._:-]*$` to
  `^[A-Za-z0-9._:/-]+$`, admitting slash-containing method names (for example `thread/read`) and
  leading punctuation; the `enabled_methods` description annotation is dropped.
- SHA-256 patterns normalize `^[a-f0-9]{64}$` → `^[0-9a-f]{64}$` (semantically identical).

**Policies, both surfaces (remain `v1`)**

- context-memory policy is byte-identical.
- provider-session policy tightens `version` and `epoch` from `minimum: 0` to `minimum: 1`, with
  title wording. The `$id` is unchanged, so this tightening is **not version-signalled**.

## Decision (proposed)

1. Treat the harness producer artifacts as authoritative for the capability descriptor and policy
   of both surfaces; copy and pin the producer bytes at a recorded harness revision.
2. Advertise `ascension.context-memory.capabilities.v3` and
   `ascension.provider-session.capabilities.v3`, publishing the required `effective_limits` and
   `binding` objects.
3. Consume `ascension.harness.effective-limits.v1`: authenticate the record against the validated
   trusted descriptor (never against the record under test), then admit a value before presenting
   it. A field absent from the record is `field_not_advertised`, never unlimited. The closed
   reasons are `effective_limit_exceeded`, `disabled`, `field_not_advertised`, `descriptor_stale`,
   `descriptor_tampered`, `profile_mismatch`, `consumer_not_recorded`, `consumer_pin_not_adopted`.
4. Keep a **dual reader** (`v1` and `v3`) during migration. Do not remove `v1` reading, and do not
   flip the advertised version, until Studio adopts `v3`.
5. Preserve this repository's own retention guard: private retention still requires an accepted
   policy exception and authenticated encryption, independent of the schema's
   `encrypted_state` widening.
6. Update the harness pin-matrix consumer entries for this repository and Studio to `aligned` with
   an exact harness CI pin in a linked harness PR.

## Compatibility classification

| Change | Class | Consumer impact | Bridge |
|---|---|---|---|
| capabilities `v1` → `v3` (`$id`/`schema` const) | Versioned, additive-but-required | Strict `v1` readers reject `v3` | Dual version-specific readers; cut over after Studio adopts `v3` |
| new required `effective_limits` | Additive (`v3` only) | Producer must populate; consumer should admit before presenting | Valid `v1` payloads omit it (both schemas are `additionalProperties: false`); the `v3` reader requires it |
| new required `binding` | Additive (`v3` only) | Consumer may ignore until it validates owner/adapter identity | Version-specific readers validate their own closed shape |
| provider-session `enabled_methods[].pattern` widening | Widening within the versioned capabilities change (`v1`→`v3`) | `v3` admits slash-containing/leading-punctuation method names that `v1` rejected | Record the method-name mapping; treat unknown methods as `unknown_methods` |
| provider-session policy `version`/`epoch` min `0`→`1` | **Unversioned tightening** | Payloads with `0` become invalid under the same `$id` | Decision required (see Open questions) |
| `hardening.encrypted_state` `const true` → `boolean` | Widening | Could weaken a naive consumer | Console keeps an independent retention guard |

No route is removed and no Console snapshot/event schema changes.

## Migration order and rollback

1. **Record this agreement** on `#24`, `sts2-harness#95`, and the Studio consumer.
2. **Console PR**: copy/pin the four producer artifacts; add the dual reader; advertise `v3` only
   after the adapter and Studio can read it; add effective-limits consumption and fail-closed
   tests.
3. **Studio PR**: accept `v3` on its `schema_adapter` surface.
4. **Harness PR**: set both consumer entries to `aligned` with CI pins.
5. **Rollback**: set the advertised version back to `v1` and keep the dual reader; the copied `v3`
   artifacts remain inert until re-cut.

## Evidence limits

Synthetic/contract-level only. The producer bytes are pinned by digest; no native, provider, or
deployment behavior is claimed. Console's delivery remains its bounded validation, ingest/storage,
scoped control API/CLI, and browser presentation.

## Resolved questions and evidence

1. **Unversioned policy tightening — accepted, no producer version bump.** Copy the producer's
   `…policy.v1` provider-session policy bytes (`version`/`epoch` `minimum: 1`). The producer contract
   is authoritative for the executable limits, the tightened minima reject only semantically invalid
   `0` values, and no Context Console policy payload uses `0` (verified across `crates/`,
   `contracts/`, and `fixtures/`). If the producer later versions the policy, consumers re-pin.
2. **Studio cutover — dual reader, then coordinated flip.** This repository keeps a version-specific
   `v1`/`v3` reader and only advertises `v3` once Studio can accept it (Studio consumer feature
   `ascension-workflow-studio#119`). The harness pin matrix moves both consumer entries to `aligned`
   after that.
3. **Encryption widening — confirmed intentional; independent guard retained.** The producer `v3`
   contract defines `hardening.encrypted_state` as a boolean. This repository does not rely on it for
   safety: private retention still requires an accepted policy exception and authenticated
   encryption (`RetentionPolicy::validate`), independent of the schema value.
4. **Method-name widening — accepted as-is.** The Console already uses slash-containing method names
   (for example `thread/read` in its provider-session fixture), which the `v1` pattern rejected; the
   `v3` pattern admits them. Unknown or malformed names remain explicit `unknown_methods`/invalid
   rather than being silently normalized.

### Verified consumer inconsistency

The copied `v1` capability schemas are not merely older versions: this repository's own
provider-session fixture already uses values the copied `v1` schema rejects (`enabled_methods`
`thread/read` violates the `v1` pattern; `hardening.encrypted_state: false` violates the `v1`
`const: true`). Replacing the copies with the producer `v3` bytes is therefore a correctness fix, not
only a version alignment.

## Consequences

Adoption aligns a real producer/consumer contract defect (current harness-produced descriptors use
`v3` and cannot be read by a strict `v1` consumer), unblocks the `#24` obligation and downstream
`#18`/`#19`, and makes value presentation depend on authenticated executable limits rather than
portable schema ceilings alone. With the decision accepted, the migration PR follows; until it
lands, this repository must not claim adoption, and the harness pin matrix must keep it `pending`.