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
| `/v2/runs/{run_id}/context-control/pause` | Latch the scheduler at a validated boundary. |
| `/v2/runs/{run_id}/context-control/commits` | CAS-commit an approved preview while remaining paused. |
| `/v2/runs/{run_id}/context-control/resume` | Explicitly release the approved continuation after revalidation. |

Control writes require the fixture editor capability, exact loopback Origin, and the CSRF token.
Objective overrides use the separate objective capability. Every command has a bounded idempotency
key and command-window identity. The journal envelope preserves drafts, revisions, receipts,
events, pause state, and selected bytes across a controller recovery; old boundary/plan epochs are
fenced. The copied schemas are in `contracts/context-control`.

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
