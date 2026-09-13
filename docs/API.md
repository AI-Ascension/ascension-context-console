# Read and control API

`context-service` exposes a small read-only HTTP surface through `ReadApi`. A caller must provide
an in-memory bearer capability, an exact `Host`, and, when configured, the exact `Origin`. The
default listener helper binds `127.0.0.1`; there is no wildcard CORS and no URL fetch.

Supported GET resources are:

| Route | Result |
| --- | --- |
| `/health` | Minimal liveness and zero provider/game counters; no capability is needed. |
| `/v1/capabilities` | Phase 1 capability and disclosure contract. |
| `/v1/runs` | Scoped run identifiers. |
| `/v1/runs/{run_id}/snapshots` | Bounded immutable snapshot summaries. |
| `/v1/runs/{run_id}/snapshots/{snapshot_id}` | Authorized snapshot manifest. |
| `/v1/runs/{run_id}/snapshots/{snapshot_id}/components/{component_id}` | Component metadata and availability state. |
| `/v1/runs/{run_id}/snapshots/{snapshot_id}/components/{component_id}/content` | Authorized retained bytes with the declared media type. |
| `/v1/runs/{run_id}/events?cursor=C` | Bounded lifecycle events with an opaque cursor authenticated for this bearer grant and run. |
| `/v1/runs/{run_id}/compare?left=A&right=B` | Bounded local comparison with no apply/preview action. |

Requests with a body, unsafe methods, traversal or percent escapes, URL credentials, unknown
routes, invalid limits, mismatched scope, expired grants, or revoked tokens are rejected. Event
sequence values and cursors are contiguous within the authorized project/run scope; producer-global
sequence positions are not exposed. A cursor from another project, grant, or run is rejected.
Producer capture loss remains an explicit `capture.gap` event.
Capability
expiry and revocation are checked against the current request time, including when one listener
serves multiple requests. Responses
carry `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`. The API returns component
metadata rather than reading an arbitrary content path. The plaintext store rejects private-mode
snapshots that reference content; private retention requires the separately approved encrypted vault
primitive and is not exposed by this API.

The Phase 2 fixture adds a separate authenticated control surface. It is served by the integrated
demo only and never shares read capabilities with writes:

| Route | Result |
| --- | --- |
| `/v2/runs/{run_id}/context-control/capabilities` | Enabled profile, supported operations, and explicit non-goals. |
| `/v2/runs/{run_id}/context-control/state` | Current pause latch, revision, plan epoch, and observed boundary. |
| `/v2/runs/{run_id}/context-control/eligible-items` | Scoped editable items and locked-item explanations. |
| `/v2/runs/{run_id}/context-control/revisions` | Immutable approved configuration history for restore selection. |
| `/v2/runs/{run_id}/context-control/drafts` | Create a versioned draft from the active revision. |
| `/v2/runs/{run_id}/context-control/drafts/{draft_id}/operations` | Apply bounded include/exclude/pin/note/objective/restore operations with draft CAS. |
| `/v2/runs/{run_id}/context-control/previews` | Build an exploratory or held-boundary immutable preview without inference. |
| `/v2/runs/{run_id}/context-control/commands` | Refresh bounded owner command receipts (array without a query; one receipt with exactly one `idempotency_key`, `reference_key`, or `reference` query). |
| `/v2/runs/{run_id}/context-control/commands/by-idempotency-key/{idempotency_key}` | Recover an owner receipt using the caller-known idempotency key after a lost write response. |
| `/v2/runs/{run_id}/context-control/pause` | Latch the scheduler at a validated boundary. |
| `/v2/runs/{run_id}/context-control/commits` | CAS-commit an approved preview while remaining paused. |
| `/v2/runs/{run_id}/context-control/resume` | Explicitly release the approved continuation after revalidation. |

Control writes require the fixture editor capability, exact loopback Origin, and the CSRF token.
Objective overrides use the separate objective capability. Every command has a bounded idempotency
key and command-window identity. The journal envelope preserves drafts, revisions, receipts,
events, pause state, and selected bytes across a controller recovery; old boundary/plan epochs are
fenced. The copied schemas are in `contracts/context-control`.

## Non-demo harness-backed composition

Issue #18 adds `HarnessBackedContextService`, a separate composition for an owner supplied through
the typed `HarnessOwnerPort`. It uses the same `/v2/runs/{run_id}/context-control/` resource names,
but does not use fixture tokens, fixture keys, or the integrated-demo state. The owner-auth value is
an injected opaque reference resolved by the owning harness; no upstream URL, provider/game
credential, or scheduler is accepted by this target.

The facade grants metadata, content, ordinary edit, objective, pause, commit, and resume
independently. Exact Host and configured Origin are checked on every request; writes also require
the injected CSRF proof. Expiry, revocation, scope, foreign-reference, and owner
unavailable/denied/stale/unknown outcomes are preserved as stable typed errors. Metadata projections
redact raw content, and the facade never invents prepared bytes or claims a fixture preview is
universal provider context. Capture is off by default; private retention requires the accepted
authenticated-encryption policy.

Consumer schemas, the auth configuration shape, deployment/rollback notes, and the external
integration gate are in [`HARNESS_FACADE.md`](HARNESS_FACADE.md).

## Phase 3 memory facade

The additive memory surface is served by the integrated fixture under `/v3/memory`. It is
permissioned separately from Phase 2 writes and delegates policy/content ownership to the
companion harness. The default target has no attached corpus or projection.

| Route | Permission/effect | Result |
| --- | --- | --- |
| `/v3/memory/capabilities` | `memory.search` / read | Versioned capability disclosure; disabled means no supported operations. |
| `/v3/memory/status` | `memory.search` / `local_read_no_inference` | Bounded generation, projection, revocation and inference counters; unavailable values are null. |
| `/v3/memory/search` | `memory.search` / `local_read_no_inference` | Closed `query.v1` body with scope, branch, cutoff, generation and limits; returns retrieval metadata or `projection_unavailable`. |
| `/v3/memory/generate` | `memory.review` / explicit generation | Target facade is intentionally unsupported until a harness adapter is attached. |
| `/v3/memory/review` | `memory.review` / review-only | Target facade is intentionally unsupported; review cannot commit or resume a run. |

The query body is capped at 4 KiB and the request envelope at 16 KiB. Query text is not placed in
the URL. Search never invokes inference; summaries require a distinct explicit job, exact source
manifest, independent review, and the existing Phase 2 approval/explicit-resume path. The schemas
and OpenAPI contract are pinned in [`contracts/context-memory`](../contracts/context-memory), and
the harness implementation is documented in its `docs/MEMORY.md`.

### Attached harness-owner composition

The default `MemoryRoute::new(scope, enabled)` and `ProviderSessionRoute::fixture(principal)`
constructors remain local fixture/unattached modes. They never imply a production corpus, provider
session, inference, compaction or game authority. Delegation is enabled only by explicitly
constructing a `HarnessOwnerComposition` with an injected implementation of the public
`HarnessOwner` trait, then passing it to `MemoryRoute::attached` or
`ProviderSessionRoute::attached` (use `attached_with_scope` for a non-fixture session scope).
There is no URL, process, native-RPC or credential field in this port.

`OwnerOperation` is a closed operation vocabulary. Memory capabilities/status/query use the
`read_search` grant; generation/review use the independent `generation_review` grant; selection
and provider-session control/compaction use the independent `control` grant. Every grant is
scope-bound, finite, host/origin-bound and (for writes) CSRF-bound. Revocation advances a shared
revocation epoch. A failed check is rejected before `HarnessOwner::call`.
Attached integrations must pass the complete `OwnerRequestContext` through
`handle_with_context`; the compatibility `handle` entry point does not reconstruct origin or CSRF
proofs for an attached route.

The owner returns an `OwnerReply` containing a versioned, metadata-only `OwnerReceipt` and an
optional bounded public value. The console forwards the receipt's source, operation, owner epoch,
evidence and `accepted`/`unknown`/`unsupported` outcome without fabricating completion. A
`LostReply` causes exactly one `lookup_receipt` call; an unknown lookup remains `unknown` and is
never retried. Read/history operations are fenced if the owner reports inference, native or game
effects. Responses carrying credentials, raw RPC/native references or private-content fields are
rejected. The real `sts2-harness` owner implementation remains an external integration gate.

The equivalent bounded operator CLI is exposed by the compiled `context-console` binary:

```text
context-console phase2-cli init <store>
context-console phase2-cli state|capabilities|eligible [--include-content]|revisions|drafts|events <store>
context-console phase2-cli draft-create <store>
context-console phase2-cli draft-edit <store> [author] < patch.json
context-console phase2-cli draft-show <store> <draft_id>
context-console phase2-cli preview <store> <draft_id> <version> <applicable> [risk_ack]
context-console phase2-cli preview-show <store> <preview_id>
context-console phase2-cli pause <store> <idempotency_key>
context-console phase2-cli commit <store> <idempotency_key> <preview_id>
context-console phase2-cli resume <store> <idempotency_key> <preview_id>
context-console phase2-cli command <store> <command_id>
```

Each mutating command reloads the encrypted journal, rechecks scope, versions, boundary, expiry,
permissions, and budget through the reducer, then persists before reporting success. The machine
JSON envelope is `ascension.context-control.cli-result.v1`; private content is redacted by default
and note bodies are accepted only from stdin. Exit code 0 means the command completed or was
accepted; 2 is usage/validation, 3 is a stale/conflict outcome, 4 is denied, and 5 is durable-store
unavailability. This fixture CLI has no provider, game, URL, or process-execution capability.
