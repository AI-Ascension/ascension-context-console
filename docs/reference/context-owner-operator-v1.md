# Context owner operator service

This document describes the private Unix operator service source interface. It does not claim that the service has been deployed or accepted by a native Harness process.

## Configuration and protected files

The closed JSON configuration uses `schema_version` `ascension.context-console.owner-service.v1` and `protected_source` `protected-private-files-v1`. It supplies numeric loopback `listen_address` and `harness_address`, one `owner_id`, the complete `scope` (`project_id`, `run_id`, `episode_id`, `agent_id`), `issuer`, `audience`, `expected_host`, optional `expected_origin`, protected-file references for `csrf_secret_ref`, `principal_registry_ref`, `principal_mac_key_ref`, `owner_slots_ref`, and `journal_key_manifest_ref`, the `state_root`, distinct `grant_database` and `journal_database` names, and `request_deadline_ms` from 1 through 5000. Unknown fields, duplicate or aliased references, non-loopback addresses, and non-absolute state roots are rejected.

The configuration file and every referenced secret, registry, slot, key-manifest, database, and SQLite sidecar are opened without following symbolic links. The configured state root is service-owned mode `0700`; secret and database files are service-owned, single-link regular files mode `0600`. Each path component is checked against unsafe writable ancestors, and the retained root identity is revalidated around state access. The fixed `owner-service.lock` is reserved and cannot be reused as a configured artifact. Startup refuses non-Unix platforms or missing, malformed, or unprotected inputs before binding. It does not create principal keys, Harness credentials, journal keys, or grants.

## Commands

All paths below must be absolute. No secret value is accepted on the command line.

```text
context-console owner serve --config ABSOLUTE_CONFIG
context-console owner grant-provision --config ABSOLUTE_CONFIG --input ABSOLUTE_INPUT
context-console owner grant-revoke --config ABSOLUTE_CONFIG --input ABSOLUTE_INPUT
context-console owner journal-key-rotate --config ABSOLUTE_CONFIG
```

Serving and each privileged command acquire the same non-blocking exclusive lock under the configured state root before opening or mutating state. A contention or lock-path replacement fails closed. `serve` writes `{"status":"ready"}` only after protected inputs, grant storage, journal storage, and the listener are ready. Provision/revoke inputs are bounded private JSON files with closed schemas `ascension.context-console.subject-grant-provision.v1` and `ascension.context-console.subject-grant-revoke.v1`. Provisioning requires the configured issuer and exact configured four-part scope; permissions are limited to the existing Console permission set. These commands use the existing grant store and do not expose an HTTP grant-issuance route.

`journal-key-rotate` is an offline/drained operation protected by the same lock. It calls the existing data-key rotation procedure and creates no key material. Retain the configured index key and all keys still needed by journal rows. Rotation does not provide a global cutover mechanism for already-running instances.

## HTTP contract

The listener accepts one bounded request per connection and serves only these additive POST routes:

| Route | Body and behavior |
| --- | --- |
| `/v2/context-owner/invocations` | One direct `ContextOwnerInvocationV2` JSON request. The typed operation selects read-only send or write reservation/send. |
| `/v2/context-owner/invocations/receipt-lookup` | A direct invocation DTO preserving the original operation and request identity for an already ambiguous supported write, with current authorization as described below; derives the exact Harness receipt lookup and never resends the write. |
| `/v2/context-owner/invocations/cached-result` | A direct invocation DTO preserving the original operation and request identity for an already completed write, with current authorization as described below; returns only the journaled response. |

The HTTP body is not a Console v1 wrapper and cannot select a permission list or admission use. The operator checks the configured owner and full scope, current authenticated principal and exact grant, CSRF proof, and the protected Harness slot descriptor. Write requests are durably reserved before sending. An ambiguous result requires exact receipt lookup; it is never permission to retry a POST. A cached result remains subject to current authorization immediately before disclosure.

Recovery preserves the original typed operation, its full request payload and stable key, Console issuer/subject/audience and complete scope, Harness actor/owner/workflow run, and expected binding. A fresh recovery request may update only its authorization envelope: Console credential ID, grant ID/generation/expiry, and Harness credential reference/expiry. Both receipt lookup and cached-result disclosure require a currently admitted `MetadataRead` grant and `workflow:read` Harness scope. A write admitted with an `Edit` grant therefore uses a separate current `MetadataRead` grant for recovery; each grant row has one permission. The operator verifies this fresh envelope before matching the stored operation and retains the resulting immutable admission throughout that request. It neither rewrites the journaled operation nor substitutes a new mutation.

Headers are limited to 8192 bytes; request and response bodies are limited to 1 MiB. The configured request lifetime is at most five seconds and applies across framing, authorization callbacks, persistence, transport, and response disclosure. Responses use the closed `HarnessResponseV1` contract. Errors are sanitized and do not return remote messages, credentials, raw claims, or filesystem paths.

The source-level gates do not establish hard preemption of synchronous SQLite or authorization callbacks; deadline checks prevent a late connection or disclosure. Revocation checks are point-in-time and do not form a transaction with Harness. A privileged restore of an older, valid encrypted journal snapshot can restore a prior prepared state; AEAD does not prevent rollback. These sources and tests do not establish deployment, production key custody, native Harness acceptance, or overall Console issue completion.
