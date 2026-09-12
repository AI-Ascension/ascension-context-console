// SPDX-License-Identifier: MIT

use super::durable::DurableControlStore;
use super::durable_schema::digest;
use super::durable_types::{
    CURRENT_DURABLE_STORE_SCHEMA_VERSION, DurableStoreError, DurableStoreSnapshot, MAX_EVENT_BYTES,
    MAX_SNAPSHOT_BYTES,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::fs;
use std::path::Path;

impl DurableControlStore {
    pub fn legacy_open(path: impl AsRef<Path>) -> Result<(), DurableStoreError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Err(DurableStoreError::Missing);
        }
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| DurableStoreError::Sqlite)?;
        let Some(version) = connection
            .query_row(
                "SELECT value FROM context_control_meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?
        else {
            return Ok(());
        };
        if version != CURRENT_DURABLE_STORE_SCHEMA_VERSION.to_string() {
            return Err(DurableStoreError::Incompatible);
        }
        let active = connection
            .query_row(
                "SELECT management_active FROM context_control_journal
                 ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?
            .unwrap_or(0);
        match active {
            0 => Ok(()),
            1 => Err(DurableStoreError::ManagementActive),
            _ => Err(DurableStoreError::Corrupt),
        }
    }

    pub fn snapshot(&self) -> Result<DurableStoreSnapshot, DurableStoreError> {
        let (management_active, journal_bytes, journal_digest) = self
            .connection
            .query_row(
                "SELECT management_active, length(envelope), envelope_digest
                 FROM context_control_journal WHERE run_id = ?1",
                [self.run_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?
            .ok_or(DurableStoreError::Missing)?;
        let management_active = match management_active {
            0 => false,
            1 => true,
            _ => return Err(DurableStoreError::Corrupt),
        };
        let mut statement = self
            .connection
            .prepare(
                "SELECT snapshot, digest FROM context_control_phase1_snapshots
                 WHERE run_id = ?1 ORDER BY snapshot_id",
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
        let mut rows = statement
            .query([self.run_id.as_str()])
            .map_err(|_| DurableStoreError::Sqlite)?;
        let mut phase1_snapshot_digests = Vec::new();
        while let Some(row) = rows.next().map_err(|_| DurableStoreError::Sqlite)? {
            let bytes = row
                .get::<_, Vec<u8>>(0)
                .map_err(|_| DurableStoreError::Sqlite)?;
            let expected = row
                .get::<_, String>(1)
                .map_err(|_| DurableStoreError::Sqlite)?;
            if bytes.len() > MAX_SNAPSHOT_BYTES || digest(&bytes) != expected {
                return Err(DurableStoreError::Corrupt);
            }
            phase1_snapshot_digests.push(expected);
        }
        drop(rows);
        let mut statement = self
            .connection
            .prepare(
                "SELECT event, event_digest FROM context_control_outbox
                 WHERE run_id = ?1 ORDER BY sequence",
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
        let mut rows = statement
            .query([self.run_id.as_str()])
            .map_err(|_| DurableStoreError::Sqlite)?;
        let mut outbox_event_digests = Vec::new();
        while let Some(row) = rows.next().map_err(|_| DurableStoreError::Sqlite)? {
            let bytes = row
                .get::<_, Vec<u8>>(0)
                .map_err(|_| DurableStoreError::Sqlite)?;
            let expected = row
                .get::<_, String>(1)
                .map_err(|_| DurableStoreError::Sqlite)?;
            if bytes.len() > MAX_EVENT_BYTES || digest(&bytes) != expected {
                return Err(DurableStoreError::Corrupt);
            }
            outbox_event_digests.push(expected);
        }
        Ok(DurableStoreSnapshot {
            run_id: self.run_id.clone(),
            management_active,
            journal_bytes: usize::try_from(journal_bytes)
                .map_err(|_| DurableStoreError::Corrupt)?,
            journal_digest,
            phase1_snapshot_count: phase1_snapshot_digests.len(),
            phase1_snapshot_digests,
            outbox_event_count: outbox_event_digests.len(),
            outbox_event_digests,
        })
    }

    pub fn copy_phase1_snapshot(
        &mut self,
        snapshot_id: &str,
        bytes: &[u8],
    ) -> Result<(), DurableStoreError> {
        if !valid_snapshot_id(snapshot_id) {
            return Err(DurableStoreError::InvalidSnapshotId);
        }
        if bytes.is_empty() || bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(DurableStoreError::TooLarge);
        }
        let content_digest = digest(bytes);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| DurableStoreError::Sqlite)?;
        let existing = transaction
            .query_row(
                "SELECT digest FROM context_control_phase1_snapshots
                 WHERE run_id = ?1 AND snapshot_id = ?2",
                params![self.run_id, snapshot_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?;
        if let Some(existing) = existing {
            if existing != content_digest {
                return Err(DurableStoreError::SnapshotConflict);
            }
        } else {
            transaction
                .execute(
                    "INSERT INTO context_control_phase1_snapshots
                        (run_id, snapshot_id, snapshot, digest) VALUES (?1, ?2, ?3, ?4)",
                    params![self.run_id, snapshot_id, bytes, content_digest],
                )
                .map_err(|_| DurableStoreError::Sqlite)?;
        }
        transaction.commit().map_err(|_| DurableStoreError::Sqlite)
    }

    pub fn backup(&self, destination: impl AsRef<Path>) -> Result<(), DurableStoreError> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(FULL)")
            .map_err(|_| DurableStoreError::Sqlite)?;
        fs::copy(&self.path, destination)
            .map(|_| ())
            .map_err(|_| DurableStoreError::Sqlite)
    }
}

fn valid_snapshot_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}
