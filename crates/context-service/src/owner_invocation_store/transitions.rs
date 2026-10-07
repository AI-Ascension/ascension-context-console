use crate::harness_context_owner_wire::{HarnessResponseV1, MAX_HARNESS_JSON_BODY_BYTES};
use rusqlite::TransactionBehavior;

use super::StoreError;
use super::crypto::{OwnerInvocationKeyProvider, encrypt};
use super::engine::{OneUseLookupPermit, OneUseSendPermit, OwnerInvocationStore};
use super::engine_helpers::advance;
use super::operations::response_from_lookup;
use super::record::EntryState;
use super::record_codec::{open_record_with_keys, seal_record_with_keys};
use super::storage::{
    MAX_CIPHERTEXT_BYTES, StoredRow, check_storage_bounds, check_write_headroom, map_sql_error,
    read_row, update_row,
};
use zeroize::Zeroizing;

impl<K: OwnerInvocationKeyProvider> OwnerInvocationStore<K> {
    /// Persist a fully validated typed owner response before returning it to Console.
    pub(crate) fn complete_send(
        &mut self,
        permit: OneUseSendPermit,
        response_bytes: &[u8],
        now: u64,
    ) -> Result<HarnessResponseV1, StoreError> {
        if response_bytes.len() > MAX_HARNESS_JSON_BODY_BYTES {
            self.mark_send_unknown(permit, now)?;
            return Err(StoreError::InvalidOwnerResponse);
        }
        let decoded = match permit.invocation.decode_response(response_bytes) {
            Ok(response) => response,
            Err(_) => {
                self.mark_send_unknown(permit, now)?;
                return Err(StoreError::InvalidOwnerResponse);
            }
        };
        let encoded = Zeroizing::new(
            serde_json::to_vec(&decoded).map_err(|_| StoreError::InvalidOwnerResponse)?,
        );
        if encoded.len() > MAX_HARNESS_JSON_BODY_BYTES {
            self.mark_send_unknown(permit, now)?;
            return Err(StoreError::InvalidOwnerResponse);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        let row = read_row(&transaction, &permit.tag)?.ok_or(StoreError::StoreCorrupt)?;
        if row.entry_id != permit.entry_id || row.sequence != permit.sequence {
            return Err(StoreError::StoreCorrupt);
        }
        let mut record = open_record_with_keys(&self.keys, &row)?;
        if record.state != EntryState::WriteClaimed
            || !super::operations::same_call(&record.invocation, &permit.invocation)
        {
            return Err(StoreError::RecoveryRequired);
        }
        record.response = Some(decoded.clone());
        advance(&mut record, EntryState::Completed, now)?;
        let next = seal_record_with_keys(&self.keys, &mut record, row.tag, row.entry_id)?;
        update_row(&transaction, &next)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(decoded)
    }

    /// Record a transport outcome whose application status is ambiguous. The only recovery path
    /// is the typed exact lookup endpoint; no retry permit is returned.
    pub(crate) fn mark_send_unknown(
        &mut self,
        permit: OneUseSendPermit,
        now: u64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        let row = read_row(&transaction, &permit.tag)?.ok_or(StoreError::StoreCorrupt)?;
        if row.entry_id != permit.entry_id || row.sequence != permit.sequence {
            return Err(StoreError::StoreCorrupt);
        }
        let mut record = open_record_with_keys(&self.keys, &row)?;
        if record.state != EntryState::WriteClaimed
            || !super::operations::same_call(&record.invocation, &permit.invocation)
        {
            return Err(StoreError::RecoveryRequired);
        }
        record.response = None;
        advance(&mut record, EntryState::Unknown, now)?;
        let next = seal_record_with_keys(&self.keys, &mut record, row.tag, row.entry_id)?;
        update_row(&transaction, &next)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_sql_error)
    }

    /// Cache a validated exact receipt. A missing mutation/publication receipt remains Unknown;
    /// a malformed lookup also remains Unknown and can never enable a write replay.
    pub(crate) fn complete_lookup(
        &mut self,
        permit: OneUseLookupPermit,
        response_bytes: &[u8],
        now: u64,
    ) -> Result<Option<HarnessResponseV1>, StoreError> {
        if response_bytes.len() > MAX_HARNESS_JSON_BODY_BYTES {
            self.mark_lookup_unknown(permit, now)?;
            return Err(StoreError::InvalidOwnerResponse);
        }
        let decoded = match permit.invocation.decode_response(response_bytes) {
            Ok(response) => response,
            Err(_) => {
                self.mark_lookup_unknown(permit, now)?;
                return Err(StoreError::InvalidOwnerResponse);
            }
        };
        let cached = match response_from_lookup(&permit.original, decoded) {
            Ok(value) => value,
            Err(_) => {
                self.mark_lookup_unknown(permit, now)?;
                return Err(StoreError::InvalidOwnerResponse);
            }
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        let row = read_row(&transaction, &permit.tag)?.ok_or(StoreError::StoreCorrupt)?;
        if row.entry_id != permit.entry_id || row.sequence != permit.sequence {
            return Err(StoreError::StoreCorrupt);
        }
        let mut record = open_record_with_keys(&self.keys, &row)?;
        if record.state != EntryState::LookupClaimed
            || !super::operations::same_call(&record.invocation, &permit.original)
        {
            return Err(StoreError::RecoveryRequired);
        }
        record.response = cached.clone();
        let next_state = if cached.is_some() {
            EntryState::Completed
        } else {
            EntryState::Unknown
        };
        advance(&mut record, next_state, now)?;
        let next = seal_record_with_keys(&self.keys, &mut record, row.tag, row.entry_id)?;
        update_row(&transaction, &next)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_sql_error)?;
        Ok(cached)
    }

    pub(crate) fn mark_lookup_unknown(
        &mut self,
        permit: OneUseLookupPermit,
        now: u64,
    ) -> Result<(), StoreError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        let row = read_row(&transaction, &permit.tag)?.ok_or(StoreError::StoreCorrupt)?;
        if row.entry_id != permit.entry_id || row.sequence != permit.sequence {
            return Err(StoreError::StoreCorrupt);
        }
        let mut record = open_record_with_keys(&self.keys, &row)?;
        if record.state != EntryState::LookupClaimed
            || !super::operations::same_call(&record.invocation, &permit.original)
        {
            return Err(StoreError::RecoveryRequired);
        }
        record.response = None;
        advance(&mut record, EntryState::Unknown, now)?;
        let next = seal_record_with_keys(&self.keys, &mut record, row.tag, row.entry_id)?;
        update_row(&transaction, &next)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_sql_error)
    }

    /// Re-encrypt every retained row atomically under the provider's current data key. The
    /// stable index ID/key cannot change here; all existing data keys remain required so retained
    /// backups and any row not yet retired remain decryptable.
    pub(crate) fn rotate_data_keys(&mut self) -> Result<(), StoreError> {
        let new_keys = self.provider.load()?;
        if !self.keys.same_index_material(&new_keys) {
            return Err(StoreError::IndexKeyRotationRequiresOfflineMigration);
        }
        if !self.keys.retains_existing_data_key_material(&new_keys) {
            return Err(StoreError::KeyUnavailable);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        let tags = read_all_tags(&transaction)?;
        let mut replacements = Vec::with_capacity(tags.len());
        for tag in tags {
            let row = read_row(&transaction, &tag)?.ok_or(StoreError::StoreCorrupt)?;
            let mut record = open_record_with_keys(&self.keys, &row)?;
            record.data_key_id = new_keys.current_data_key_id().to_owned();
            let serialized = Zeroizing::new(
                serde_json::to_vec(&record).map_err(|_| StoreError::StoreUnavailable)?,
            );
            if serialized.len() > MAX_CIPHERTEXT_BYTES {
                return Err(StoreError::StorageLimit);
            }
            let (nonce, ciphertext) = encrypt(
                &new_keys,
                &row.tag,
                &row.entry_id,
                row.state as i64,
                row.sequence,
                &serialized,
            )?;
            if ciphertext.len() > MAX_CIPHERTEXT_BYTES {
                return Err(StoreError::StorageLimit);
            }
            replacements.push(StoredRow {
                tag: row.tag,
                entry_id: row.entry_id,
                state: row.state,
                sequence: row.sequence,
                data_key_id: new_keys.current_data_key_id().to_owned(),
                nonce,
                ciphertext,
            });
        }
        for row in &replacements {
            update_row(&transaction, row)?;
        }
        check_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_sql_error)?;
        self.keys = new_keys;
        self.validate_all_rows()
    }
}

fn read_all_tags(connection: &rusqlite::Connection) -> Result<Vec<[u8; 32]>, StoreError> {
    let mut statement = connection
        .prepare("SELECT lookup_tag FROM owner_invocations ORDER BY lookup_tag")
        .map_err(|_| StoreError::StoreUnavailable)?;
    statement
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(|_| StoreError::StoreUnavailable)?
        .map(|result| {
            result
                .map_err(|_| StoreError::StoreCorrupt)?
                .try_into()
                .map_err(|_| StoreError::StoreCorrupt)
        })
        .collect()
}
