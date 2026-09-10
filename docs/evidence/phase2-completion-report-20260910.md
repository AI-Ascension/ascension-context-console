# Phase 2 completion and evidence handoff

This is the review record for the Phase 2 draft branches. Phase 1 was merged first, then the existing target repositories were extended; no Phase 2 branch was merged, released, deployed, or used against a live provider/game.

Target: [ascension-context-console PR #3](https://github.com/AI-Ascension/ascension-context-console/pull/3), implementation source `b6617a53a8f81ea72e61fe4113f5fe1d20245c4d`, evidence handoff pending the final evidence commit, final branch head pending the metadata reconciliation commit. Companion: [sts2-harness PR #60](https://github.com/AI-Ascension/sts2-harness/pull/60) at `4c79e83d27691e265a4649b7361a111f617c3f79`. The target and companion heads are cross-linked in both PR bodies. Contract source pins and SHA-256 values are in `contracts/context-control/README.md`.

The implementation is disabled outside the synthetic management fixture. The target control plane never calls a provider or game; the companion seam proves prepared bytes through fake Exo/Astra/Ollama peers. Real provider, host, deployment and soak evidence is not claimed.

## Gates and demonstrations

The target and companion each passed `cargo fmt --all -- --check`, strict repository policy, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`, `cargo test --workspace --all-targets --all-features --locked`, and locked cargo metadata. The target `phase2-demo` passed with `provider_calls=0` and `game_launches=0`; its prepared manifest is `734b79c24a125e465e72be6004193345bc2aa3f6e290e18df88e49a19d316f9b`. Loopback API probes passed capabilities/state, CSRF rejection, duplicate-key rejection, and a valid write. The package verifier, schema fixtures (15 schemas, 17 valid vectors, 28 rejected vectors), and bounded reference model passed.

The Phase 2 browser audit now passes with the bounded Chromium/Playwright environment: it exercises
draft save, note/objective entry, typed pinning and restore, exploratory and applicable previews,
pause, commit-while-paused, explicit resume, control events, authorization denial probes, zero
provider/game effects, storage, network, keyboard and narrow-layout assertions. Native depth-2/3
Luna/max descendants and reservation metadata remain unavailable;
see `docs/evidence/phase2-native-preflight-20260910.json`.

The checked-in `docs/evidence/phase2-failure-matrix-20260910.json` enumerates all 80 package
failure rows. Eight rows have executable evidence: seven browser cases (`P2-F001`, `P2-F003`,
`P2-F005`, `P2-F011`, `P2-F017`, `P2-F022`, and `P2-F031`) plus the Rust resume guard
(`P2-F039`); the other 72 remain explicitly marked
`required_not_executed` because the fixture has no dedicated fault injection or live provider,
storage, migration, and process-ownership harness for them.

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
| P2-R016 | source-reviewed | Edit-time and preview-time immutable scope/digest/expiry checks are implemented. |
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
| P2-R031 | source-reviewed | Plan epoch and controller fencing reject obsolete work. |
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
| P2-R051 | source-reviewed | CLI demo uses the same typed control reducer; a separate remote CLI is not claimed. |
| P2-R052 | executed | Relations bind revision, preview, snapshot, execution, attempt and intervention IDs. |
| P2-R053 | source-reviewed | Intervention lineage is retained on revisions and relations after restore. |
| P2-R054 | unverified | The fixture is in-memory; no native storage migration rehearsal is claimed. |
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

The 14 target Phase 2 integration tests cover protected edits, draft CAS, objective authorization,
typed pin/exclude/restore, protected identity aliases, exploratory/applicable preview,
pause/commit/resume, stale boundaries, idempotent receipts, command-window expiry, changed-body
conflicts, journal tamper/recovery, disabled mode, and stop dominance. The companion tests cover
prepared Exo bytes, legacy parity, controller recovery, invalid UTF-8/expiry/pin rejection, and
actual serialized Ollama/Astra bridge inputs. The package’s bounded reference model additionally
checked 12,389 transitions and eight targeted scenarios across 1,802 states; it does not validate
Rust storage, authentication, provider behavior, game effects, or native topology. The package
failure matrix has 80 rows. The row-level ledger records eight executed cases and 72
`required_not_executed` rows; scenarios requiring production storage, live providers, native
orchestration, or unavailable fault injection remain outside the evidence boundary.

No production migration or rollback claim is made because this fixture uses bounded in-memory state;
the additive journal recovery and safe deactivation behavior are tested, while a native encrypted
store migration and old-binary refusal still require implementation. Schema fixtures were executed
with `jsonschema`; native depth/reservation controls remain unavailable in this environment. A
maintainer should review the two draft PRs and rerun the documented gates before release work.
