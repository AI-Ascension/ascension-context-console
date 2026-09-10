# Evidence classes

Every result has a bounded evidence class:

| Class | Meaning |
| --- | --- |
| `source` | A local source/build/test fact from this checkout or its pinned dependency. |
| `synthetic` | A checked-in fixture or fake-only demonstration; no live service is implied. |
| `native` | A platform-specific process, filesystem, or browser observation. The checked-in browser record is a local Chromium observation; native storage and platform coverage remain separate. |
| `provider` | A real provider request or receipt. No real provider call is made by this delivery. |
| `deployment` | A remote repository, review, merge, or release action. No merge or release is implied. |

The accepted harness source pin used by the fixtures is
`fc44d3ef65fefa6d13ecd5f690e5335a6ef60080`. Current local Rust gates are source evidence. The
offline CLI and browser fixtures are synthetic evidence. The companion harness production source
`316c8bd1814d9f9762a08c534898ec827365c91a` and the companion evidence branch (machine-readable
evidence introduced at `1136255`) record bounded
Astra and Ollama process runs against synthetic fake downstreams; those runs
match the accepted baseline at the captured handoff and failure boundaries. No real provider or
game receipt, native storage check, or cross-platform behavior is implied. Local Chromium runs are
recorded in `docs/evidence/browser-ui-20260910.json` and
`docs/evidence/integrated-browser-ui-20260910.json` with desktop and narrow screenshots. The
integrated record additionally covers the synthetic producer, memory capture, authenticated API
projection, and browser request counters; both runs use loopback requests only.

The target's `phase2-durable-store-20260910.json` is local component evidence for the opt-in
encrypted SQLite control journal: transaction rollback, authentication/tamper rejection, additive
schema repair with retained Phase 1 bytes, backup, and legacy active-state refusal. It does not
establish production key management, native ownership fencing, or crash/deployment behavior.
