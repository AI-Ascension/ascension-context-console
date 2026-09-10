# Context Console repository instructions

This repository is a private Phase 1 read-only inspector. The harness owns request assembly,
capture records, provider boundaries and decision lineage. This repository owns bounded validation,
restricted ingest/storage, read API/CLI and browser presentation.

Do not add provider/game credentials, arbitrary process/URL access, context editing, apply,
pause/resume, compaction, retrieval, persistent sessions, map merge or provider calls. Capture is
default-off; private retention requires an accepted policy exception and authenticated encryption.
Use Rust 1.97.1, locked builds, bounded inputs, typed errors, no unsafe code and no secrets.

Every claim is labeled by evidence class. Synthetic fixtures prove only local behavior. Keep
manifest records immutable and append lifecycle/retention evidence. Use explicit paths and
feature branches; shared manifests, lockfiles, contract pins, CI and integration wiring are
root-owned.
