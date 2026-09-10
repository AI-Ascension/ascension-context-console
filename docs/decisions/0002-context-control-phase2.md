# Decision: typed Phase 2 context control

Status: accepted for the Phase 2 fixture branch

The target keeps the Phase 1 read store and adds a separate control authority. Operators edit
versioned drafts that refer to immutable, scoped item bytes. A pure renderer builds an ordered
prepared-input manifest, and previews retain that exact material without invoking a provider.
Only a held pause boundary can produce an applicable preview. Commit uses the current revision,
control version, preview digest, and boundary as a compare-and-swap; it remains paused. Resume is
an explicit command and rechecks the continuation boundary.

The target stores a bounded JSON journal envelope for recovery. Recovery increments the controller
epoch and retains pause, revisions, receipts, events, selected bytes, and prepared material.
Duplicate commands return the original receipt when the body matches; a changed body with the same
idempotency key conflicts. The companion harness performs provider dispatch through the exact
prepared bytes and remains authoritative for game state and action legality.

The fixture intentionally supports text items only. Images, compaction, retrieval, persistent
provider sessions, model/account switching, and direct game actions remain unsupported.
