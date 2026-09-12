# Implemented Phase 3 handoff

Date: 2026-09-11. This is the additive Phase 3 handoff on `phase2/context-editing`, merged to
target `main` by PR #3. The target implementation is covered by target revision
`8e52da837ae0a23cea18d7cd3d5164765911e6b6`; the
companion harness revision is `3cb7296`.

The target extends the existing Phase 1/2 checkout. `crates/context-service/src/memory.rs`
contains the target-owned `MemoryRoute`, `MemoryQueryRequest`, capability disclosure, bounded
search body checks, and separate search/review principals. It owns no corpus, provider, game,
process, URL, or scheduler. `cli.rs::run_phase3_cli` invokes the same bounded target facade for
`capabilities`, `status`, and `search`; `memory.rs::MemoryRoute::handle` serves the authenticated
`/v3/memory` paths beside the unchanged `/v2` control routes.

The browser files (`web/index.html`, the `web/js/` modules, and `web/css/styles.css`) expose disabled,
projection-unavailable, generation, revocation, review-required, and held-approval states. Query
text and returned snippets use inert text nodes. The browser never persists capabilities or resolves
fixture paths. Existing Phase 1/2 panels and bytes remain covered by their original tests and
audits.

The target copies the eleven versioned `context-memory` schemas/OpenAPI file and the synthetic
source/map fixtures under `contracts/context-memory/` and `fixtures/context-memory/`. Their
contract owner is the companion harness; this copy is a byte-pinned consumer artifact. The target
README, API, demo, testing notes, and ADR state the disabled default, explicit effect classes,
limits, rollback boundary, and unavailable live/native lanes.

The default target CLI and integrated demo keep the facade disabled until a corpus/projection
generation is attached; their capability response labels unattached retrieval and compaction lanes
`unverified` and exposes no generate, review, policy, or adoption operation. The explicit
`phase3-adapter` command is a bounded offline integration fixture: it accepts the reviewed selection,
checks the exact Phase 2 prepared-manifest identity, and writes the memory binding in the same target
SQLite transaction as the paused Phase 2 revision. The executable demo then invokes the fake peer,
holds approval, and submits one exact first resume. This does not attach a production projection or
claim live provider/game behavior. Do not replace the Phase 2 store or add a target-side summary
scheduler.

## Contract and seam table

| Seam | Implementation | Contract/effect | Owner | Current evidence | Compatibility risk |
| --- | --- | --- | --- | --- | --- |
| Capability/status | `memory.rs::MemoryRoute::handle` | `capabilities.v1`, status; read/local-read | target facade | `tests/phase3_memory.rs` | unavailable values must stay explicit |
| Search | `memory.rs::MemoryRoute::handle` | `query.v1` → `retrieval.v1`; `local_read_no_inference` | harness policy, target route | target phase3 tests; browser route | adapter must bind scope/cutoff/generation |
| CLI | `cli.rs::run_phase3_cli` | `context-memory.cli-result.v1` | target | target phase3 tests | must remain equivalent to HTTP validation |
| Corpus/policy | companion `context_memory` module | entry/query/retrieval/policy/selection v1 | harness | companion phase3 tests | no target duplicate store |
| Summary/review | companion `SummaryJobStore`, `MemoryReview` | summary-job/proposal/review v1 | harness | companion phase3 tests; `../phase2-harness/docs/evidence/phase3-summary-isolation-20260911.json` | fake peer only; live provider unverified |
| Phase 2 adoption | `cli.rs::run_phase3_adapter` plus durable binding table | selection/approval v1 + existing control v1 | target adapter + existing Phase 2 owner | `docs/evidence/phase3-adapter-demo-20260911.json`; `tests/phase2_durable.rs` | offline fixture only; native/game path remains separate |
| Revocation | companion corpus/revocation records | revocation v1; deny before cleanup | harness | companion revocation test | restore/backup ordering unverified |
| Map | companion `MapBundle::validate` and `MapMemoryGate` | original synthetic map generation fence plus current legal authority | harness/target adapter | map policy tests | no current game authority |

## Required next adapter checks

Before enabling the feature, extend the offline adapter proof with stale/revoked dependency fencing,
provider-write unknown recovery, and a production projection attachment. Keep live provider, native
game, deployment, and native three-level orchestration as separate evidence classes; the independent
native three-level reviewer remains unavailable in this handoff.
