// SPDX-License-Identifier: MIT

//! Opt-in encrypted SQLite persistence for the target control fixture.
//!
//! The journal is encrypted before SQLite sees it. Control callers persist a candidate plane and
//! only publish it to their live projection after the transaction commits. This is a fixture seam
//! with explicit local durability evidence; it does not claim native deployment guarantees.

use super::durable_schema::{digest, ensure_schema, insert_outbox, now_seconds};
use super::durable_types::{AAD, DurableStoreError, DurableStoreFailpoint, MAX_JOURNAL_BYTES};
use super::state::ControlPlane;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};

pub struct DurableControlStore {
    pub(super) path: PathBuf,
    pub(super) key: [u8; 32],
    pub(super) run_id: String,
    pub(super) connection: Connection,
    pub(super) failpoint: Option<DurableStoreFailpoint>,
}

impl DurableControlStore {
    pub fn create(
        path: impl AsRef<Path>,
        key: [u8; 32],
        plane: &ControlPlane,
    ) -> Result<Self, DurableStoreError> {
        let run_id = plane.scope().run_id.clone();
        let mut store = Self::open_connection(path.as_ref(), key, run_id)?;
        store.persist(plane)?;
        Ok(store)
    }

    pub fn open(
        path: impl AsRef<Path>,
        key: [u8; 32],
        run_id: impl Into<String>,
    ) -> Result<Self, DurableStoreError> {
        Self::open_connection(path.as_ref(), key, run_id.into())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn set_failpoint(&mut self, failpoint: Option<DurableStoreFailpoint>) {
        self.failpoint = failpoint;
    }

    pub fn persist(&mut self, plane: &ControlPlane) -> Result<(), DurableStoreError> {
        if plane.scope().run_id != self.run_id {
            return Err(DurableStoreError::ScopeMismatch);
        }
        let journal = plane
            .export_journal()
            .map_err(|_| DurableStoreError::Encode)?;
        if journal.len() > MAX_JOURNAL_BYTES {
            return Err(DurableStoreError::TooLarge);
        }
        let envelope = self.encrypt(&journal)?;
        let envelope_digest = digest(&envelope);
        if self.failpoint == Some(DurableStoreFailpoint::BeforeJournalWrite) {
            self.failpoint = None;
            return Err(DurableStoreError::Failpoint);
        }
        let state = plane.state();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| DurableStoreError::Sqlite)?;
        transaction
            .execute(
                "INSERT INTO context_control_journal
                    (run_id, envelope, envelope_digest, management_active, active_revision_id,
                     control_version, pause_latched, stop_latched, controller_epoch, gate_epoch,
                     plan_epoch, last_sequence, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(run_id) DO UPDATE SET
                    envelope = excluded.envelope,
                    envelope_digest = excluded.envelope_digest,
                    management_active = excluded.management_active,
                    active_revision_id = excluded.active_revision_id,
                    control_version = excluded.control_version,
                    pause_latched = excluded.pause_latched,
                    stop_latched = excluded.stop_latched,
                    controller_epoch = excluded.controller_epoch,
                    gate_epoch = excluded.gate_epoch,
                    plan_epoch = excluded.plan_epoch,
                    last_sequence = excluded.last_sequence,
                    updated_at = excluded.updated_at",
                params![
                    self.run_id,
                    envelope,
                    envelope_digest,
                    i64::from(plane.enabled()),
                    state.active_revision_id,
                    state.control_version as i64,
                    i64::from(state.pause_latched),
                    i64::from(state.stop_latched),
                    state.controller_epoch as i64,
                    state.gate_epoch as i64,
                    state.plan_epoch as i64,
                    state.last_sequence as i64,
                    now_seconds(),
                ],
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
        insert_outbox(&transaction, &self.run_id, &plane.events())?;
        if self.failpoint == Some(DurableStoreFailpoint::BeforeCommit) {
            self.failpoint = None;
            return Err(DurableStoreError::Failpoint);
        }
        transaction.commit().map_err(|_| DurableStoreError::Sqlite)
    }

    pub fn load(&self) -> Result<ControlPlane, DurableStoreError> {
        let (
            envelope,
            envelope_digest,
            management_active,
            active_revision_id,
            control_version,
            paused,
            stopped,
            controller_epoch,
            gate_epoch,
            plan_epoch,
            last_sequence,
        ) = self
            .connection
            .query_row(
                "SELECT envelope, envelope_digest, management_active, active_revision_id,
                        control_version, pause_latched, stop_latched, controller_epoch,
                        gate_epoch, plan_epoch, last_sequence
                 FROM context_control_journal WHERE run_id = ?1",
                [self.run_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?
            .ok_or(DurableStoreError::Missing)?;
        if envelope.len() > MAX_JOURNAL_BYTES || digest(&envelope) != envelope_digest {
            return Err(DurableStoreError::Corrupt);
        }
        let journal = self.decrypt(&envelope)?;
        let plane =
            ControlPlane::recover_journal(&journal).map_err(|_| DurableStoreError::Decode)?;
        let state = plane.state();
        if state.scope.run_id != self.run_id
            || i64::from(plane.enabled()) != management_active
            || state.active_revision_id != active_revision_id
            || state.control_version as i64 != control_version
            || i64::from(state.pause_latched) != paused
            || i64::from(state.stop_latched) != stopped
            || state.controller_epoch as i64 != controller_epoch.saturating_add(1)
            || state.gate_epoch as i64 != gate_epoch
            || state.plan_epoch as i64 != plan_epoch
            || state.last_sequence as i64 != last_sequence
        {
            return Err(DurableStoreError::Corrupt);
        }
        Ok(plane)
    }

    pub fn management_active(&self) -> Result<bool, DurableStoreError> {
        let value = self
            .connection
            .query_row(
                "SELECT management_active FROM context_control_journal WHERE run_id = ?1",
                [self.run_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|_| DurableStoreError::Sqlite)?
            .ok_or(DurableStoreError::Missing)?;
        match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DurableStoreError::Corrupt),
        }
    }

    fn open_connection(
        path: &Path,
        key: [u8; 32],
        run_id: String,
    ) -> Result<Self, DurableStoreError> {
        if path.as_os_str().is_empty() || run_id.is_empty() {
            return Err(DurableStoreError::InvalidPath);
        }
        if key.iter().all(|byte| *byte == 0) {
            return Err(DurableStoreError::InvalidKey);
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            return Err(DurableStoreError::ParentMissing);
        }
        let connection = Connection::open(path).map_err(|_| DurableStoreError::Sqlite)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = FULL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
            .map_err(|_| DurableStoreError::Sqlite)?;
        ensure_schema(&connection)?;
        Ok(Self {
            path: path.to_owned(),
            key,
            run_id,
            connection,
            failpoint: None,
        })
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, DurableStoreError> {
        let mut nonce = [0_u8; 24];
        getrandom::getrandom(&mut nonce).map_err(|_| DurableStoreError::AuthenticationFailed)?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.key));
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: AAD,
                },
            )
            .map_err(|_| DurableStoreError::AuthenticationFailed)?;
        let mut envelope = Vec::with_capacity(nonce.len() + ciphertext.len());
        envelope.extend_from_slice(&nonce);
        envelope.extend_from_slice(&ciphertext);
        Ok(envelope)
    }

    fn decrypt(&self, envelope: &[u8]) -> Result<Vec<u8>, DurableStoreError> {
        if envelope.len() < 24 + 16 {
            return Err(DurableStoreError::Corrupt);
        }
        let (nonce, ciphertext) = envelope.split_at(24);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.key));
        let plaintext = cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: AAD,
                },
            )
            .map_err(|_| DurableStoreError::AuthenticationFailed)?;
        if plaintext.len() > MAX_JOURNAL_BYTES {
            return Err(DurableStoreError::TooLarge);
        }
        Ok(plaintext)
    }
}
