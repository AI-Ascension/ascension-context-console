# Implemented Phase 3 handoff

Date: 2026-09-11. This is the additive Phase 3 draft on `phase2/context-editing`. The target
implementation is covered by target revision `b21083719fb825be83088c9a6ab09531463a1a72`; the
companion harness revision is `f801ee8`.

The target extends the existing Phase 1/2 checkout. `crates/context-service/src/memory.rs`
contains the target-owned `MemoryRoute`, `MemoryQueryRequest`, capability disclosure, bounded
search body checks, and separate search/review principals. It owns no corpus, provider, game,
process, URL, or scheduler. `cli.rs::run_phase3_cli` invokes the same bounded target facade for
`capabilities`, `status`, and `search`; `integrated_demo.rs::memory_request` serves the authenticated
`/v3/memory` paths beside the unchanged `/v2` control routes.

The browser files (`web/index.html`, `web/app.js`, and `web/styles.css`) expose disabled,
projection-unavailable, generation, revocation, review-required, and held-approval states. Query
text and returned snippets use inert text nodes. The browser never persists capabilities or resolves
fixture paths. Existing Phase 1/2 panels and bytes remain covered by their original tests and
audits.

The target copies the eleven versioned `context-memory` schemas/OpenAPI file and the synthetic
source/map fixtures under `contracts/context-memory/` and `fixtures/context-memory/`. Their
contract owner is the companion harness; this copy is a byte-pinned consumer artifact. The target
README, API, demo, testing notes, and ADR state the disabled default, explicit effect classes,
limits, rollback boundary, and unavailable live/native lanes.

The companion seam is an explicit integration dependency. The target CLI and integrated demo keep
the facade disabled until an adapter supplies corpus/projection generations; the capability
response labels unattached retrieval and compaction lanes `unverified` and exposes no generate,
review, policy or adoption operation. Attach the harness policy through a future adapter only after preserving the existing Phase 2 renderer, prepared-manifest identity,
pause/commit/resume serializer, action lineage, and permission boundary. Do not replace the Phase 2
store or add a target-side summary scheduler. The current target returns `projection_unavailable`
when no harness projection is attached; it does not claim an enabled end-to-end memory run.

## Contract and seam table

| Seam | Implementation | Contract/effect | Owner | Current evidence | Compatibility risk |
| --- | --- | --- | --- | --- | --- |
| Capability/status | `memory.rs::MemoryRoute::handle` | `capabilities.v1`, status; read/local-read | target facade | `tests/phase3_memory.rs` | unavailable values must stay explicit |
| Search | `memory.rs::MemoryRoute::handle` | `query.v1` → `retrieval.v1`; `local_read_no_inference` | harness policy, target route | target phase3 tests; browser route | adapter must bind scope/cutoff/generation |
| CLI | `cli.rs::run_phase3_cli` | `context-memory.cli-result.v1` | target | target phase3 tests | must remain equivalent to HTTP validation |
| Corpus/policy | companion `context_memory` module | entry/query/retrieval/policy/selection v1 | harness | companion phase3 tests | no target duplicate store |
| Summary/review | companion `SummaryJobStore`, `MemoryReview` | summary-job/proposal/review v1 | harness | companion phase3 tests | fake peer only; live adapter unverified |
| Phase 2 adoption | existing control serializer; target route does not mutate it | selection/approval v1 + existing control v1 | existing Phase 2 owner | existing phase2 tests/audits; approval unit path | exact first request and commit-held identity need adapter |
| Revocation | companion corpus/revocation records | revocation v1; deny before cleanup | harness | companion revocation test | restore/backup ordering unverified |
| Map | companion `MapBundle::validate` | original synthetic map generation fence | harness/target adapter | unit coverage | no current game authority |

## Required next adapter checks

Before enabling the feature, run the real harness/target adapter against synthetic fake peers and
prove exact first-input bytes, whole-input budget, stale/revoked dependency fencing, provider-write
unknown recovery, and explicit resume. Keep live provider, native game, deployment, and native
three-level orchestration as separate evidence classes.
