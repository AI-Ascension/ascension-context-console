# ADR 0021: Internal owner capability publication and scoped trust

Accepted for the Console source port by the coordinating owner, 2026-09-14. This implements
ADR 0020's consumer gate. The cutover PR must remain draft until Studio PR132's v3 reader is
merged and the coordinator's exact-head review/CI gates pass.

## Existing boundary and change

At harness `f8015e52ccb530e60d722283ef2b063da372169b`, memory and session libraries can emit
`ascension.harness.effective-limits.v1` records. Memory's CLI has a separate `limits` command;
this is not a Console management transport. No session record transport or attached Console
adapter is inferred from those library methods.

The Console-owned, in-process `HarnessOwner` port retains the existing operations
`memory.capabilities` and `provider_session.capabilities`. Their accepted `OwnerReply` can now
carry an optional typed `effective_limits` sidecar containing the producer's actual v1 record.
It sits outside the closed capability descriptor in `value`; public descriptor and receipt
schemas are unchanged. No operation, HTTP route, CLI command or wire protocol is added.
Constructors retain `None` by default; struct-literal adapters must initialize the new field.
Only accepted capability replies may carry it, and combined descriptor/record bytes remain
bounded by the owner response limit.

## Authority and presentation

Composition receives `MemoryCapabilityTrust` or `SessionCapabilityTrust` from independent
configuration, never from the reply being checked. Trust binds the exact descriptor and its
owner/adapter/policy revisions, full project/run/episode/agent scope, receipt source and owner
epoch. Descriptors must satisfy the copied v3 bounds/identities and producer-compatible payload
digest. Self-consistent hashing alone is not authority.

V3 capability presentation requires the supplied sidecar and independent trust. It authenticates
the complete producer record, admits each presented executable limit, validates the returned
descriptor and requires equality with the selected descriptor. Missing trust returns
`consumer_not_recorded`; a v1 owner descriptor at a v3 route returns `consumer_pin_not_adopted`;
missing record returns `field_not_advertised`. Tampering, stale epoch/digest, wrong scope/profile
and over-limit values return their closed machine-readable reasons. The Console never derives a
replacement record and calls it owner-published.

Memory queries additionally use a fresh capability read with the same read grant to admit the
three disclosed input limits: results, candidates and query bytes. Rejected values cause zero
`memory.query` calls. The grant is checked again after the capability read so revocation stops
the query. Generation and provider/session policy execution remain harness-owned.

Disabled memory remains discoverable with `enabled: false`; query admission returns `disabled`.
An empty-method session descriptor is not valid v3 and cannot be configured as an attachable
profile. Unknown method names are not normalized or given execution authority.

## Compatibility, rollback and evidence

Default capability presentation is v3 after this coordinated cutover. Both readers retain v1.
`with_capability_version(CapabilityVersion::V1)` explicitly restores legacy presentation with no
`binding` or `effective_limits`; its consumer pin is pending with limits unavailable. The legacy
owner-delegation tests exercise this rollback, while `capability_routes` exercises default v3.
The local default v3 consumer disclosure is aligned; it does not alter the authoritative harness
pin matrix, which needs a separate owner PR with exact consumer and supported CI pins.

Local synthetic capabilities use real payload/adapter digest semantics, not schema-digest
stand-ins. Attached routes do not substitute those fixtures for an owner's publication.
Default-off capture and independent private-retention approval/encryption guards remain intact.

The injected owner tests consume the original producer-library vectors from PR28 and prove the
consumer's actual route/admission boundary. Production adapter population of this sidecar remains
unimplemented under Console #18/#19 and harness #100. No real owner, native peer, provider, game,
deployment or newly available transport is claimed by this consumer cutover.
