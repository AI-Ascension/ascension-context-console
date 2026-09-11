# Phase 4 migration and rollback boundary

Provider-session metadata is additive to the Phase 1 snapshot, Phase 2 control journal and Phase 3
memory records. The broker-owned metadata adapter stores only the bounded session journal; it does
not rewrite earlier evidence or copy native rollout files. The encrypted adapter authenticates its
envelope and requires exact scope, policy and capability-profile matches before restore. A mismatch
is rejected, so an older reader cannot silently activate a newer native profile.

Restore is ordered and held: the owner epoch rotates, retirement/revocation tombstones are applied,
nonterminal turn and interrupt operations become `unknown`, and no prepared bytes are resumed. The
normal Phase 2 commit and explicit-resume gates remain required before any executable binding can
become active. A failed or interrupted restore is discarded; retrying the same metadata read is safe
because the authenticated envelope and idempotency records are unchanged.

Rollback or downgrade must first stop admission, fence the current owner and reconcile unknown
operations. The replacement reader must pass the checked scope/policy/profile comparison. If it
cannot understand the active provider profile, it remains read-only/held or uses a reviewed export;
it must not ignore active bindings, revive tombstoned state, resend a turn or mutate native files.
Switching back to the stateless path is an explicit retirement/deactivation operation, not a hot
swap. Existing stateless Phase 1–3 behavior remains available when provider-session mode is disabled.

The source/fixture tests cover authenticated metadata restore, scope/policy/profile drift rejection,
owner rotation, unknown-turn recovery and tombstone ordering. Native-binary migration, encrypted
native WAL/log/temp containment, downgrade compatibility and provider-side cleanup remain
unverified because the installed native profile is not eligible for persistent activation.
