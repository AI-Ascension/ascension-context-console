# Decision: encrypted durable control store

Status: accepted for the Phase 2 fixture branch

The integrated target fixture persists its bounded control journal in a separate SQLite database.
The journal envelope is encrypted with XChaCha20-Poly1305 before it is written, authenticated with
a versioned associated-data label, and accompanied by a SHA-256 integrity digest. SQLite runs in
WAL mode with `synchronous=FULL`, foreign keys, a five-second busy timeout, and `BEGIN IMMEDIATE`
for journal, outbox, and Phase 1 snapshot updates.

Mutation handlers clone the current `ControlPlane`, apply the typed command to that candidate, and
persist the candidate plus its event outbox in one transaction. The live projection is replaced only
after commit succeeds. A failpoint before commit therefore leaves both the previous journal and
outbox unchanged. Recovery authenticates and decodes the journal, checks the denormalized state
columns, preserves pause and stop latches, and advances the controller epoch before accepting new
commands.

Phase 1 snapshot bytes are copied by immutable `(run_id, snapshot_id, digest)` identity. A partial
additive schema is repaired by `CREATE TABLE IF NOT EXISTS`; missing journal data is not invented,
newer or mismatched schema markers are refused, and a legacy read-only opener refuses a
management-active database. Full WAL checkpoint and file-copy backup are exposed for the owned
fixture.

The store is intentionally opt-in and local to the integrated synthetic process. It does not claim
multi-process ownership fencing, native reservation evidence, production key management, provider
or game recovery, deployment rollback, or process-crash coverage beyond the deterministic
transaction failpoint and reopen tests recorded in the Phase 2 evidence.
