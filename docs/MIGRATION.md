# Phase 2 migration and safe deactivation

Phase 2 is an additive control layer over the Phase 1 read store. Existing snapshot and lifecycle
event records remain owned by `Store`; the control journal is a separate bounded envelope and never
derives active revision state from lossy capture. A Phase 1 bundle can therefore be opened and
inspected before and after enabling the fixture without rewriting its bytes.

For the offline fixture, run the compatibility check and the control recovery check:

```text
cargo test --locked --package context-service --test phase1_acceptance
cargo test --locked --package context-service --test phase2_control
cargo run --locked --package context-service --bin context-console -- demo
cargo run --locked --package context-service --bin context-console -- phase2-demo
```

`ControlPlane::export_journal` writes the bounded Phase 2 journal. `ControlPlane::recover_journal`
validates its schema and event limit, restores selected immutable bytes and receipts, and advances
the controller epoch before any new command can be accepted. Recovery keeps a held pause and an
active revision; it does not auto-resume or resend an unknown provider effect.

Safe deactivation is explicit. Call `ControlPlane::deactivate` (or construct a disabled plane for a
new fixture) before removing the management surface. Deactivation retains the active revision,
plan epoch, pause/stop latch, journal, and Phase 1 read projections while rejecting new draft,
preview, commit, pause, and resume writes with `management_disabled`. It does not silently fall
back to a different revision or discard control records. Re-enable only through an explicit
`ControlPlane::activate` operation after the control policy and capability have been reviewed.

This repository has no production database migration or deployment script: the implementation is a
bounded in-memory fixture, and live storage, provider, host, and release rollback remain outside
its evidence boundary. A production rollout must first provide an encrypted content store,
transactional journal migration, outbox recovery, and an old-binary refusal policy before treating
this fixture runbook as an operational migration.
