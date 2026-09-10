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
game receipt, native storage check, or cross-platform behavior is implied. A local Chromium run is
recorded in `docs/evidence/browser-ui-20260910.json` with desktop and narrow screenshots; it uses
only the checked-in synthetic bundle and loopback requests.
