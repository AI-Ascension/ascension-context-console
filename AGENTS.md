# Context Console repository instructions

This repository contains the Phase 1 read-only inspector and its Phase 2 controlled-context
extension. The harness owns request assembly, capture records, provider boundaries, control
authority and decision lineage. This repository owns bounded validation, restricted ingest/storage,
the scoped control API/CLI, and browser presentation.

Phase 2 may add draft editing, deterministic preview, durable pause/commit/resume and recovery
records through the typed control boundary. It must not add provider/game credentials, arbitrary
process/URL access, compaction, retrieval, persistent provider sessions, map merge or direct game
actions. Capture remains default-off; private retention requires an accepted policy exception and
authenticated encryption. Use Rust 1.97.1, locked builds, bounded inputs, typed errors, no unsafe
code and no secrets.

Every claim is labeled by evidence class. Synthetic fixtures prove only local behavior. Keep
manifest records immutable and append lifecycle/retention evidence. Use explicit paths and
feature branches; shared manifests, lockfiles, contract pins, CI and integration wiring are
root-owned.

## Workspace, branch, and artifact hygiene

Before creating an isolated checkout, declare the exact absolute worktree path and the exact branch name. Create it only with `git worktree add <absolute-path> -b <branch-name>` (or attach the explicitly named existing branch). Do not create branch copies, sibling checkouts, backup trees, archive trees, or `*-tmp*` directories as substitutes for a Git worktree; do not use generated or random paths for branch isolation.

Perform edits and validation only in the declared checkout. Put build output, test fixtures, logs, and other derived artifacts in the repository's designated rebuildable output directory (such as `target/`) or a single declared task scratch path, never beside repositories or directly under `/home/agent`. Remove task scratch/output after it is no longer needed, and remove the worktree with `git worktree remove <the-same-absolute-path>` once its branch is integrated or abandoned. Preserve source, committed evidence, and any path explicitly retained by the task owner.
