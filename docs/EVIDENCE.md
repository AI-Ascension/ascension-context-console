# Evidence classes

Every result has a bounded evidence class:

| Class | Meaning |
| --- | --- |
| `source` | A local source/build/test fact from this checkout or its pinned dependency. |
| `synthetic` | A checked-in fixture or fake-only demonstration; no live service is implied. |
| `native` | A platform-specific process, filesystem, or browser observation. None is claimed by the offline demo. |
| `provider` | A real provider request or receipt. No real provider call is made by this delivery. |
| `deployment` | A remote repository, review, merge, or release action. No merge or release is implied. |

The accepted harness source pin used by the fixtures is
`fc44d3ef65fefa6d13ecd5f690e5335a6ef60080`. Current local Rust gates are source evidence. The
offline CLI and browser fixtures are synthetic evidence. The companion harness commit `f3746ed`
also records bounded Astra and Ollama process runs against synthetic fake downstreams; those runs
match the accepted baseline at the captured handoff and failure boundaries. No real provider or
game receipt, browser execution, native storage check, or independent review is implied.
