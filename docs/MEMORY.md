# Phase 3 memory operator notes

The Phase 3 memory namespace is additive to the frozen Phase 2 control API. The target facade
serves `GET /v3/memory/capabilities`, `GET /v3/memory/status`, and bounded `POST /v3/memory/search`
requests. Search bodies use `ascension.context-memory.query.v1`, carry a run/episode/agent scope,
branch, causal cutoff, corpus generation, ranker version and `local_read_no_inference` effect
class. Queries are posted in the body so private text is not placed in a URL. Responses disclose
`projection_unavailable` while no harness projection is attached.

The equivalent fixture command is:

```text
cargo run --locked --package context-service --bin context-console -- phase3-cli capabilities
cargo run --locked --package context-service --bin context-console -- phase3-cli status
printf '%s' '<query.v1 JSON>' | cargo run --locked --package context-service --bin context-console -- phase3-cli search
```

The target fixture defaults memory to disabled. It exposes no generate route that can start model
work, and it never writes the harness corpus directly. The harness’s `context_memory` module is the
policy owner; its fake summary peer is only an offline external-boundary test. A reviewed proposal
is still separate from admission, selection, Phase 2 commit, and explicit resume. Revocation fences
source descendants before any bounded cleanup.

The UI shows capability, generation, revocation, retrieval coverage and compaction lifecycle states
with text and keyboard-accessible controls. Rendered source and query text is inserted with
`textContent`; no source URL or browser persistence is accepted. Unsupported vector retrieval,
persistent provider sessions, provider-side compaction, hidden reasoning, live provider quality,
native game execution and deployment remain unavailable.
