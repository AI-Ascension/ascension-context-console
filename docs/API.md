# Read API

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
| `/v1/runs/{run_id}/events?cursor=N` | Bounded lifecycle events, reconnect cursor, and gap flag. |
| `/v1/runs/{run_id}/compare?left=A&right=B` | Bounded local comparison with no apply/preview action. |

Requests with a body, unsafe methods, traversal or percent escapes, URL credentials, unknown
routes, invalid limits, mismatched scope, expired grants, or revoked tokens are rejected. Responses
carry `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`. The API returns component
metadata rather than reading an arbitrary content path; private content authorization is a separate
policy-controlled primitive.

The checked-in OpenAPI document is a proposed contract seed. The Rust route table is the executable
Phase 1 implementation and intentionally has no ingest or management endpoint.
