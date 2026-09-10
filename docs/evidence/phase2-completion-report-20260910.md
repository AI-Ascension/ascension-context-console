# Phase 2 completion and evidence handoff

This is the review record for the Phase 2 draft branches. Phase 1 was merged first, then the existing target repositories were extended; no Phase 2 branch was merged, released, deployed, or used against a live provider/game.

Target: [ascension-context-console PR #3](https://github.com/AI-Ascension/ascension-context-console/pull/3), implementation source `46167a20b8d63d0b570c987a532fad1d05b88e69`, evidence handoff `1dade34c5f6f6c03fa3a55cdb3d52ff2c971b03f`. Companion: [sts2-harness PR #60](https://github.com/AI-Ascension/sts2-harness/pull/60) at `7580619964ad73fd107031fb5bf6a1a6f5c9e3ec`. The target and companion heads are cross-linked in both PR bodies. Contract source pins and SHA-256 values are in `contracts/context-control/README.md`.

The implementation is disabled outside the synthetic management fixture. The target control plane
never calls a provider or game; its integrated fixture now starts from an opt-in encrypted SQLite
journal, reopens it, and persists each successful management mutation before publishing the live
projection. The companion seam proves prepared bytes through fake Exo/Astra/Ollama peers and
exercises its own opt-in encrypted SQLite journal with local migration, rollback, and deactivation
tests. Real provider, host, deployment and soak evidence is not claimed.

## Gates and demonstrations

The target and companion each passed `cargo fmt --all -- --check`, strict repository policy, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, `cargo test --workspace --all-targets --all-features --locked`, and locked cargo metadata. The target `phase2-demo` passed with `provider_calls=0` and `game_launches=0`; its prepared manifest is `734b79c24a125e465e72be6004193345bc2aa3f6e290e18df88e49a19d316f9b`. Loopback API probes passed capabilities/state, CSRF rejection, duplicate-key rejection, and a valid write. The package verifier, schema fixtures (15 schemas, 17 valid vectors, 28 rejected vectors), and bounded reference model passed.

The Phase 2 browser audit now passes with the bounded Chromium/Playwright environment: it exercises
draft save, note/objective entry, typed pinning and restore, exploratory and applicable previews,
pause, commit-while-paused, explicit resume, control events, authorization denial probes, zero
provider/game effects, the target durable-store capability, storage, network, keyboard and narrow-layout assertions. Native depth-2/3
Luna/max descendants and reservation metadata remain unavailable;
see `docs/evidence/phase2-native-preflight-20260910.json`.

The compiled operator CLI is covered by [`docs/evidence/phase2-cli-20260910.json`](phase2-cli-20260910.json):
separate processes run the encrypted durable edit/preview/pause/commit/resume sequence, default
content redaction, explicit content permission, typed stdin editing, duplicate-key rejection, and
the 16 KiB input bound. The CLI emits the documented machine schema and makes zero provider, game,
or external requests.

The checked-in `docs/evidence/phase2-failure-matrix-20260910.json` enumerates all 80 package
failure rows. Forty-four rows have executable evidence: the browser cases (`P2-F001`,
`P2-F003`, `P2-F004`, `P2-F005`, `P2-F007`, `P2-F010`, `P2-F011`, `P2-F012`, `P2-F017`,
`P2-F022`, `P2-F031`, `P2-F073`, `P2-F074`, `P2-F075`, and `P2-F076`) plus named Rust
regressions for CAS, protected identity, restore, authorization, preview freshness/expiry,
idempotency, plan fencing, stop dominance, safe deactivation, and the approved-resume guard, plus
target durable-store regressions for transaction rollback, wrong-key/tamper rejection, additive
schema repair with Phase 1 byte retention, legacy active-state refusal, and companion owner fencing
for `P2-F064`, plus the companion expired-pin and compiled fake-peer bridge regressions. The other 36 remain
explicitly marked `required_not_executed` because they require crash windows not injected here,
live provider/game effects, native ownership controls, or other evidence outside this fixture.

## Requirement index

Statuses mean: **executed** has a local executable assertion or gate; **source-reviewed** is implemented and documented but lacks a dedicated executable assertion; **unverified** is an explicit environment or product boundary gap.

| Requirement | Status | Evidence |
| --- | --- | --- |
| P2-R001 | source-reviewed | Phase1 pins in `contracts/context-control/README.md`; target/harness branches reuse merged mains. |
| P2-R002 | unverified | Native preflight records that the requested Luna/max depth-2/3 descendants were unavailable. |
| P2-R003 | unverified | Native preflight records unavailable native reservation controls. |
| P2-R004 | executed | Target Phase1 acceptance tests and full workspace gate pass. |
| P2-R005 | executed | Harness context-control legacy test plus Exo/Ollama compatibility tests. |
| P2-R006 | executed | Harness compiled Exo, Astra, and Ollama bridge tests pass against fake peers. |
| P2-R007 | executed | Contract copies and SHA-256 pins match in both repositories. |
| P2-R008 | source-reviewed | Architecture/decision records keep provider/game authority in the harness/host boundary. |
| P2-R009 | executed | Target revision/journal and Phase1 immutability tests pass. |
| P2-R010 | executed | `protected_edit_and_draft_cas_fail_without_mutating_the_draft`. |
| P2-R011 | executed | Protected-item operation is rejected without draft mutation. |
| P2-R012 | executed | Bounded control types, strict JSON, duplicate-key and contract tests pass. |
| P2-R013 | executed | Versioned notes are attributed in target state and rendered harness context. |
| P2-R014 | executed | `objective_authorization_and_restore_alone_are_enforced`. |
| P2-R015 | executed | Restore is exclusive and creates a new draft configuration; target test and browser flow pass. |
| P2-R016 | executed | Target preview freshness/expiry checks plus the companion renderer digest-substitution regression bind edit bytes to immutable references. |
| P2-R017 | executed | Pins remain subject to selection, item, and aggregate component bounds; target test and browser flow pass. |
| P2-R018 | executed | Exploratory/applicable preview distinction and provider-free test pass. |
| P2-R019 | executed | Prepared renderer and dispatch seam tests pass. |
| P2-R020 | source-reviewed | Boundary, revision, adapter, model/configuration and manifest fields are bound in code. |
| P2-R021 | executed | Compiled Exo test asserts the approved prepared bytes are sent once. |
| P2-R022 | executed | `preview_pause_commit_resume_fences_the_old_plan_and_reuses_receipts` asserts the prepared continuation is cleared after the one submitted input. |
| P2-R023 | source-reviewed | Unsupported images/provider-added context/direct game dispatch are explicitly rejected or excluded. |
| P2-R024 | source-reviewed | Unavailable, protected, non-UTF8 and expired content fail closed in render paths. |
| P2-R025 | source-reviewed | Mandatory protected state is rendered before optional selected content; over-limit input fails. |
| P2-R026 | source-reviewed | Provider-added context is explicitly `not_exposed`; no fabricated token occupancy is emitted. |
| P2-R027 | executed | Pause latch and durable pause event are covered by target/harness control tests. |
| P2-R028 | executed | Pause/commit/plan admission tests fence new work. |
| P2-R029 | source-reviewed | Unresolved-operation fields block readiness; no clear-ledger path exists. |
| P2-R030 | executed | Applicable preview requires paused-ready and quiescent state. |
| P2-R031 | executed | `p2_f051_old_plan_epoch_is_rejected_after_commit` asserts plan-epoch fencing after commit. |
| P2-R032 | executed | Explicit no-edit resume path is covered by the target state machine. |
| P2-R033 | executed | Commit checks control, revision, preview and boundary CAS. |
| P2-R034 | source-reviewed | Commit/resume recheck content, boundary and authorization guards. |
| P2-R035 | source-reviewed | In-memory fixture applies revision, plan epoch, state and journal events in one reducer path. |
| P2-R036 | executed | Commit test asserts `revision_committed` while `paused` and zero provider/game effects. |
| P2-R037 | executed | Old-plan rejection is asserted after commit. |
| P2-R038 | executed | Resume requires an explicit command and approved continuation. |
| P2-R039 | executed | `stop_latch_dominates_resume_without_changing_the_revision`. |
| P2-R040 | executed | Same-key idempotency and changed-body conflict are asserted. |
| P2-R041 | executed | Journal recovery preserves receipts and prevents duplicate reapplication. |
| P2-R042 | executed | Recovery preserves pause and increments controller epoch. |
| P2-R043 | source-reviewed | Journal serialization includes retained item bytes and bounded event limits. |
| P2-R044 | executed | Separate editor/objective tokens and read/write route checks are covered by API probes. |
| P2-R045 | executed | Exact loopback Origin/CSRF and duplicate-key rejection probes pass. |
| P2-R046 | source-reviewed | Browser uses textContent/no storage; metrics omit note/input bodies. |
| P2-R047 | source-reviewed | Finite limits cover commands, notes, items, components, previews, events and journal. |
| P2-R048 | source-reviewed | Phase1 memory capture remains explicitly separate from durable control journal. |
| P2-R049 | executed | Receipt effects distinguish pause, commit, resume and preview state. |
| P2-R050 | executed | Phase 2 Playwright audit covers control/error UI, typed restore, and same-origin security assertions; disabled/disconnected live state remains outside the fixture. |
| P2-R051 | executed | Compiled `phase2_cli` process tests cover the typed durable CLI workflow, redaction, explicit capabilities, and parser bounds; no remote CLI is claimed. |
| P2-R052 | executed | Relations bind revision, preview, snapshot, execution, attempt and intervention IDs. |
| P2-R053 | source-reviewed | Intervention lineage is retained on revisions and relations after restore. |
| P2-R054 | executed | `phase2_durable` repairs the additive schema while retaining seeded Phase 1 bytes; target evidence is in `docs/evidence/phase2-durable-store-20260910.json`. |
| P2-R055 | executed | Safe deactivation preserves revision, pause, plan epoch and journal; writes fail closed. |
| P2-R056 | executed | Target fault/state tests and bounded reference model cover ordering and rejection transitions. |
| P2-R057 | executed | Compiled Astra/Ollama/Exo fake peers verify final serialized input. |
| P2-R058 | executed | Target demo and control tests report zero provider calls/game launches for management operations. |
| P2-R059 | executed | Phase 2 Playwright audit verifies network, storage, keyboard, reduced-motion and narrow layout. |
| P2-R060 | executed | This report, evidence JSON, pinned contracts, clean commits and draft PRs reconcile the handoff. |
| P2-R061 | executed | README/API/DEMO/USAGE/TESTING/decision docs give bounded run and recovery instructions. |
| P2-R062 | executed | Report separates local, synthetic-process, browser, native and live evidence classes. |
| P2-R063 | source-reviewed | Offline bundle routes have no control capability and management routes require scoped bearer auth. |
| P2-R064 | source-reviewed | Architecture distinguishes lossy Phase1 capture from authoritative control journal. |

## Failure and recovery coverage

The 15 target control tests plus 17 row-level failure tests cover protected edits, draft CAS,
objective authorization, typed pin/exclude/restore, protected identity aliases,
exploratory/applicable preview, pause/commit/resume, stale boundaries, idempotent receipts,
command-window expiry, changed-body conflicts, journal tamper/recovery, disabled mode, and stop
dominance. The six target durable-store tests cover encrypted reopen, wrong-key and tamper
rejection, transaction rollback with outbox atomicity, additive migration, legacy refusal, and
immutable snapshot backup. The companion tests cover
prepared Exo bytes, legacy parity, controller recovery, invalid UTF-8/expiry/pin rejection, actual
serialized Ollama/Astra bridge inputs, and eight durable-store migration/integrity cases, including
replacement-owner fencing for old live handles, documented in the [companion evidence record](https://github.com/AI-Ascension/sts2-harness/blob/18c0682ab1218b71db2e76a80703e676caeddd2f/docs/evidence/context-control-store-20260910.md).
The compiled target CLI suite adds six tests for durable command sequencing, explicit content
permission, stdin parsing bounds, unavailable-store exit classification, and no-mutation rejection
paths.
The package’s bounded reference model additionally
checked 12,389 transitions and eight targeted scenarios across 1,802 states; it does not validate
Rust storage, authentication, provider behavior, game effects, or native topology. The package
failure matrix has 80 rows. The row-level ledger records 44 executed cases and 36
`required_not_executed` rows; scenarios requiring production storage, live providers, native
orchestration, or unavailable fault injection remain outside the evidence boundary.

No target production migration or rollback claim is made. The target’s encrypted SQLite migration,
rollback, backup, and legacy-active refusal are local component evidence in the integrated fixture;
native filesystem behavior, multi-process ownership fencing, and production old-binary refusal still
require implementation. The companion’s additive encrypted journal recovery and safe deactivation
behavior are separate local component evidence. Schema fixtures were executed
with `jsonschema`; native depth/reservation controls remain unavailable in this environment. A
maintainer should review the two draft PRs and rerun the documented gates before release work.
