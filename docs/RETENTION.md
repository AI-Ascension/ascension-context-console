# Retention and limits

Phase 1 keeps the collector in process and bounded. The default store accepts at most 128 snapshot
manifests of at most 1 MiB each, 4,096 lifecycle events, 128 components per manifest, 128 mappings,
and 128 changed component IDs in one comparison. Event pages return at most 200 records. The
producer capture ring defaults to 128 records and 1 MiB per record; overflow evicts the oldest
record and reports a gap counter.

Memory capture has no WAL, temporary file, disk fallback, or browser persistence. Private capture
uses the authenticated `PrivateVault` primitive with a 16 MiB object bound and a 512 MiB quota;
construction fails when policy approval, restricted authorization, or a nonzero key is absent. The
vault in this delivery is in memory, so it does not claim crash recovery or a persistent filesystem
retention scheduler.

Snapshots are immutable. Expiry, queue loss, write failure, usage, and lineage are represented by
append-only events or explicit status fields. Source/provider files are never deleted by this
repository. A future persistent collector must add an independently reviewed TTL, active-reference
and tombstone implementation before making durable retention claims.
