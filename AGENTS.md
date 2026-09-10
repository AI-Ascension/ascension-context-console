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
