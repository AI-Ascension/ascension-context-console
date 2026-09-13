# Harness-backed Context Console facade

This document describes the non-demo composition added for issue #18. It is a consumer-facing
contract and deployment note, not evidence that the companion harness owner is already wired in
production. The concrete owner implementation remains an external dependency tracked by
[sts2-harness#100](https://github.com/AI-Ascension/sts2-harness/issues/100).

## Compatibility classification

This is an additive, non-demo composition. Existing Phase 1 reads, fixture Phase 2 routes, schemas,
tokens, and integrated-demo behavior are unchanged. The new `/v2` adapter reuses the existing
typed control records but adds a versioned facade capability/error/auth artifact; the
`HarnessOwnerPort` is a provisional typed seam until the owner contract is accepted. No legacy
reader is asked to interpret the new facade schemas.

## Ownership and port

`context-service::HarnessOwnerPort` is the only control seam. It accepts the existing typed
`context-control` records (`Scope`, `Draft`, `Patch`, `Preview`, `Command`, and `Receipt`) and an
opaque `ProtectedAuthReference`. The owner resolves that reference inside its own protected
authority. The port has no scheduler, provider, game, process, URL, credential, compaction, or
automatic-resume method. The owner remains responsible for request assembly, prepared bytes,
pause/commit authority, provider boundaries, and decision lineage.

`HarnessOwnerClient` injects the owner and reference. `HarnessBackedContextService` adds the
consumer boundary and can be used directly in an in-process listener via `handle_http_at` (or
`handle_http` for the current epoch). The legacy `demo`/`integrated-demo` routes remain a separate
fixture composition and continue to advertise fixture-only behavior.

The facade forwards only operations advertised by the owner capability projection:

- metadata: capabilities, state, eligible items, revisions, drafts, previews, and receipts;
- control: draft creation, CAS patching (include/exclude/pin/unpin/note/restore), deterministic
  preview, held commit, pause, explicit resume, and receipt reads.

The facade never returns the owner's prepared input bytes. Preview manifests and component
references are owner-issued; the facade capability projection reports provider-added context as
`unknown` (the existing preview record keeps its `not_exposed` vocabulary), and an exact preview is
reported as `owner_conditional` only when the owner advertises that capability.

## Authentication and authorization

The `HarnessFacadeConfig` contains a scope, exact expected Host, optional exact Origin, a digest of
an injected CSRF proof, and a retention policy. `ProtectedAuthReference` and `SecretDigest` reject
paths, URLs, credentials, and empty values. Raw bearer/CSRF values are held only in the request
envelope, whose `Debug` output redacts them; they must not be serialized or placed in a URL. The
auth schema is a deployment envelope: its `owner_auth_ref` and `csrf_digest_ref` are resolved into
the client's `ProtectedAuthReference` and `SecretDigest` before constructing the Rust config.

`GrantRegistry` stores only bearer digests. Grants are independent and scoped to the complete
project/run/episode/agent tuple:

| Grant | Allows |
| --- | --- |
| `context.metadata.read` | metadata projections and receipt reads |
| `context.content.read` | raw eligible content, only when metadata is also granted |
| `context.edit` | draft creation and ordinary CAS edits |
| `context.objective.edit` | objective replacement in addition to `context.edit` |
| `context.pause` | pause and held/applicable preview requests |
| `context.commit` | held commit |
| `context.resume` | explicit resume |

Every request checks exact Host and configured Origin before owner forwarding. Writes additionally
require the injected CSRF proof. Expired, revoked, missing, mismatched-scope, malformed, and
foreign references fail closed. The local reference index is populated from owner metadata so an
unknown item, draft, preview, revision, or receipt is rejected before its mutating owner operation
is attempted.

## Retention and capture

The facade defaults to `RetentionMode::Off`; it does not capture provider traffic. Selecting
`PrivateEncrypted` is accepted only with an explicit policy acceptance and authenticated-encryption
flag. Actual private bytes remain outside this facade and must use the existing approved encrypted
vault primitive. No fallback to plaintext is provided.

## Consumer transport

The typed HTTP adapter uses the existing versioned paths under
`/v2/runs/{run_id}/context-control/`. It requires `Authorization: Bearer ...`, `X-Principal`,
exact `Host`/`Origin`, and `X-CSRF-Token` on writes. Secret query fields (`token`,
`authorization`, or `csrf`), duplicate security headers, traversal, absolute targets, duplicate
query keys, and bodies over the 16 KiB facade JSON bound are rejected. Responses are
`Cache-Control: no-store` JSON; error bodies carry only a stable code and retryability flag.

The machine-readable consumer artifacts are:

- [`harness-facade-capabilities.schema.json`](../contracts/context-control/harness-facade-capabilities.schema.json)
- [`harness-facade-error.schema.json`](../contracts/context-control/harness-facade-error.schema.json)
- [`harness-facade-auth.schema.json`](../contracts/context-control/harness-facade-auth.schema.json)
- [`harness-facade.openapi.json`](../contracts/context-control/harness-facade.openapi.json)

The OpenAPI document references the existing Phase 2 command/draft/preview/receipt schemas. The
auth schema deliberately names opaque references rather than embedding a secret.

## Deployment and rollback notes

1. The owning process constructs the scoped owner port and injects an owner-auth reference and
   CSRF digest from its protected secret/configuration mechanism. Do not add a fixed fixture token,
   provider/game credential, upstream URL, or second scheduler.
2. Bind the listener through the owning same-origin boundary, set the exact Host/Origin values, and
   keep retention off unless the accepted encrypted-retention policy is present.
3. Advertise the returned `forwarded_operations` and preserve `unknown`, `unavailable`, `denied`,
   `stale`, and `expired` outcomes without translating them into success. Studio should treat a
   held commit and resume as separate transitions.
4. For rollback, revoke the affected grants and disable the owner capability. Keep the immutable
   owner journal/receipts for reconnect inspection; never auto-resume a paused run. Reconstructing
   a facade over the recovered owner is safe because owner idempotency returns the original receipt
   without repeating the effect.

The Rust process/composition tests use a recording synthetic owner. They prove local delegation,
scope and grant fencing, metadata redaction, same-origin transport checks, and receipt recovery.
They are synthetic evidence only; real harness lifecycle wiring, provider/native behavior, and
deployment compatibility remain unverified until the external owner gate passes.
