// SPDX-License-Identifier: MIT

use super::durable_types::{
    CURRENT_DURABLE_STORE_SCHEMA_VERSION, DurableStoreError, MAX_EVENT_BYTES, MAX_EVENTS,
    STORE_SCHEMA,
};
use super::types::Event;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn ensure_schema(connection: &Connection) -> Result<(), DurableStoreError> {
    let has_metadata = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'context_control_meta'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|_| DurableStoreError::Sqlite)?
        .is_some();
    if !has_metadata {
        let transaction = connection
            .unchecked_transaction()
            .map_err(|_| DurableStoreError::Sqlite)?;
        transaction
            .execute_batch(SCHEMA)
            .map_err(|_| DurableStoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO context_control_meta(key, value) VALUES ('schema', ?1)",
                [STORE_SCHEMA],
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO context_control_meta(key, value) VALUES ('schema_version', ?1)",
                [CURRENT_DURABLE_STORE_SCHEMA_VERSION.to_string()],
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
        transaction
            .commit()
            .map_err(|_| DurableStoreError::Sqlite)?;
        return Ok(());
    }
    let schema = connection
        .query_row(
            "SELECT value FROM context_control_meta WHERE key = 'schema'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| DurableStoreError::Sqlite)?
        .ok_or(DurableStoreError::Incompatible)?;
    let version = connection
        .query_row(
            "SELECT value FROM context_control_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| DurableStoreError::Sqlite)?
        .ok_or(DurableStoreError::Incompatible)?
        .parse::<i64>()
        .map_err(|_| DurableStoreError::Incompatible)?;
    if schema != STORE_SCHEMA || version != CURRENT_DURABLE_STORE_SCHEMA_VERSION {
        return Err(DurableStoreError::Incompatible);
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| DurableStoreError::Sqlite)?;
    transaction
        .execute_batch(SCHEMA)
        .map_err(|_| DurableStoreError::Sqlite)?;
    transaction.commit().map_err(|_| DurableStoreError::Sqlite)
}

pub(super) fn insert_outbox(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    events: &[Event],
) -> Result<(), DurableStoreError> {
    if events.len() > MAX_EVENTS {
        return Err(DurableStoreError::TooLarge);
    }
    for event in events {
        let bytes = serde_json::to_vec(event).map_err(|_| DurableStoreError::Encode)?;
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(DurableStoreError::TooLarge);
        }
        let event_digest = digest(&bytes);
        let existing = transaction
            .query_row(
                "SELECT event_digest FROM context_control_outbox
                 WHERE run_id = ?1 AND sequence = ?2",
                params![run_id, event.sequence as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?;
        if let Some(existing) = existing {
            if existing != event_digest {
                return Err(DurableStoreError::Corrupt);
            }
            continue;
        }
        transaction
            .execute(
                "INSERT INTO context_control_outbox
                    (run_id, sequence, event, event_digest, published)
                 VALUES (?1, ?2, ?3, ?4, 0)",
                params![run_id, event.sequence as i64, bytes, event_digest],
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
    }
    Ok(())
}

pub(super) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS context_control_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS context_control_journal (
    run_id TEXT PRIMARY KEY NOT NULL,
    envelope BLOB NOT NULL,
    envelope_digest TEXT NOT NULL,
    management_active INTEGER NOT NULL CHECK (management_active IN (0, 1)),
    active_revision_id TEXT NOT NULL,
    control_version INTEGER NOT NULL,
    pause_latched INTEGER NOT NULL CHECK (pause_latched IN (0, 1)),
    stop_latched INTEGER NOT NULL CHECK (stop_latched IN (0, 1)),
    controller_epoch INTEGER NOT NULL,
    gate_epoch INTEGER NOT NULL,
    plan_epoch INTEGER NOT NULL,
    last_sequence INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS context_control_outbox (
    run_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    event BLOB NOT NULL,
    event_digest TEXT NOT NULL,
    published INTEGER NOT NULL CHECK (published IN (0, 1)),
    PRIMARY KEY (run_id, sequence),
    FOREIGN KEY (run_id) REFERENCES context_control_journal(run_id)
);
CREATE TABLE IF NOT EXISTS context_control_phase1_snapshots (
    run_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    snapshot BLOB NOT NULL,
    digest TEXT NOT NULL,
    PRIMARY KEY (run_id, snapshot_id),
    FOREIGN KEY (run_id) REFERENCES context_control_journal(run_id)
);
"#;
