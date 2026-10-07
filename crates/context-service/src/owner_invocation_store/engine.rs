use crate::harness_context_owner_wire::{
    ContextOwnerEndpointV1, ContextOwnerInvocationV2, HarnessResponseV1,
    MAX_HARNESS_JSON_BODY_BYTES,
};
use rusqlite::{Connection, TransactionBehavior};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use super::StoreError;
use super::crypto::{OwnerInvocationKeyMaterial, OwnerInvocationKeyProvider, lookup_tag};
use super::engine_helpers::{
    advance, ensure_response_headroom, map_admission_error, push_attempt,
    validate_admission_current,
};
use super::operations::{admission_matches, lookup_invocation, stable_locator};
use super::record::{
    AdmissionUse, EntryState, InvocationRecord, LookupFamily, TrustedInvocationAdmission,
};
use super::record_codec::{
    check_exact_match, open_record_with_keys, seal_record_with_keys, validate_record,
};
use super::storage::{
    MAX_ROWS, check_storage_bounds, check_write_headroom, initialize_index_id, insert_row,
    map_sql_error, open_database, read_row, row_count, update_row, validate_schema,
};
use super::transitions;

pub(crate) struct OwnerInvocationStore<K: OwnerInvocationKeyProvider> {
    pub(super) connection: Connection,
    pub(super) database_path: PathBuf,
    pub(super) provider: K,
    pub(super) keys: OwnerInvocationKeyMaterial,
}

pub(crate) enum ReservationOutcome {
    Ready(ExactReservation),
    LookupRequired(ExactReservation),
    Cached(HarnessResponseV1),
}

pub(crate) struct ExactReservation {
    pub(super) tag: [u8; 32],
    pub(super) entry_id: [u8; 16],
    pub(super) family: LookupFamily,
}

pub(crate) struct OneUseSendPermit {
    pub(super) tag: [u8; 32],
    pub(super) entry_id: [u8; 16],
    pub(super) sequence: u64,
    pub(super) invocation: ContextOwnerInvocationV2,
    pub(super) endpoint: ContextOwnerEndpointV1,
    pub(super) body: Zeroizing<Vec<u8>>,
    pub(super) reference: crate::harness_facade::ProtectedAuthReference,
}

pub(crate) struct OneUseLookupPermit {
    pub(super) tag: [u8; 32],
    pub(super) entry_id: [u8; 16],
    pub(super) sequence: u64,
    pub(super) original: ContextOwnerInvocationV2,
    pub(super) invocation: ContextOwnerInvocationV2,
    pub(super) endpoint: ContextOwnerEndpointV1,
    pub(super) body: Zeroizing<Vec<u8>>,
    pub(super) reference: crate::harness_facade::ProtectedAuthReference,
}

impl<K: OwnerInvocationKeyProvider> OwnerInvocationStore<K> {
    /// Open the dedicated store only after the injected operator provider returns all required
    /// keys. `provider` has no production default or configuration in this module.
    pub(crate) fn open(path: impl AsRef<Path>, mut provider: K) -> Result<Self, StoreError> {
        if !cfg!(unix) {
            return Err(StoreError::UnsupportedPlatform);
        }
        let keys = provider.load()?;
        let (mut connection, database_path) = open_database(path.as_ref())?;
        initialize_index_id(&mut connection, &database_path, &keys)?;
        validate_schema(&connection)?;
        let mut store = Self {
            connection,
            database_path,
            provider,
            keys,
        };
        store.validate_all_rows()?;
        Ok(store)
    }

    /// Reserve one stable operation key before any owner socket call. Reuse resolves by blind
    /// index and then compares the full typed operation, binding, principal, and scope.
    pub(crate) fn reserve(
        &mut self,
        invocation: &ContextOwnerInvocationV2,
        admission: &TrustedInvocationAdmission,
        now: u64,
    ) -> Result<ReservationOutcome, StoreError> {
        invocation.validate().map_err(|_| StoreError::Invalid)?;
        validate_admission_current(admission, invocation, admission.use_kind, now)?;
        let locator = stable_locator(invocation).map_err(map_admission_error)?;
        let tag = lookup_tag(self.keys.index_key(), &locator)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        let locked_now =
            validate_admission_current(admission, invocation, admission.use_kind, now)?;
        check_write_headroom(&transaction, &self.database_path)?;
        if let Some(row) = read_row(&transaction, &tag)? {
            let record = open_record_with_keys(&self.keys, &row)?;
            check_exact_match(&record, invocation, admission)?;
            let outcome = match record.state {
                EntryState::Completed if admission.use_kind == AdmissionUse::CachedRead => {
                    ReservationOutcome::Cached(record.response.ok_or(StoreError::StoreCorrupt)?)
                }
                EntryState::Completed => return Err(StoreError::Denied),
                EntryState::Prepared if admission.use_kind == AdmissionUse::Write => {
                    ReservationOutcome::Ready(ExactReservation {
                        tag,
                        entry_id: row.entry_id,
                        family: locator.family,
                    })
                }
                EntryState::Prepared => return Err(StoreError::Denied),
                // Retained only to decode a legacy versioned state. No source path can
                // reinterpret an unsealed transport assertion as permission to retry.
                EntryState::DefinitelyNotSent => return Err(StoreError::RecoveryRequired),
                EntryState::WriteClaimed | EntryState::LookupClaimed | EntryState::Unknown
                    if locator.family.has_exact_lookup()
                        && admission.use_kind == AdmissionUse::ExactLookup =>
                {
                    ReservationOutcome::LookupRequired(ExactReservation {
                        tag,
                        entry_id: row.entry_id,
                        family: locator.family,
                    })
                }
                EntryState::WriteClaimed | EntryState::LookupClaimed | EntryState::Unknown => {
                    return Err(if locator.family.has_exact_lookup() {
                        StoreError::Denied
                    } else {
                        StoreError::AmbiguousWithoutExactLookup
                    });
                }
            };
            validate_admission_current(admission, invocation, admission.use_kind, now)?;
            transaction.commit().map_err(map_sql_error)?;
            validate_admission_current(admission, invocation, admission.use_kind, now)?;
            return Ok(outcome);
        }
        let write_now =
            validate_admission_current(admission, &admission.invocation, AdmissionUse::Write, now)?;
        if row_count(&transaction)? >= MAX_ROWS {
            return Err(StoreError::Capacity);
        }
        let endpoint = invocation.endpoint().map_err(|_| StoreError::Invalid)?;
        let body = invocation
            .harness_body()
            .map_err(|_| StoreError::Invalid)?
            .ok_or(StoreError::Invalid)?;
        if body.len() > MAX_HARNESS_JSON_BODY_BYTES {
            return Err(StoreError::StorageLimit);
        }
        let mut entry_id = [0_u8; 16];
        getrandom::getrandom(&mut entry_id).map_err(|_| StoreError::CryptoUnavailable)?;
        let mut record = InvocationRecord {
            schema_version: super::record::RECORD_SCHEMA_VERSION,
            state: EntryState::Prepared,
            sequence: 1,
            data_key_id: self.keys.current_data_key_id().to_owned(),
            origin: admission.snapshot.clone(),
            invocation: invocation.clone(),
            endpoint,
            canonical_body: String::from_utf8(body).map_err(|_| StoreError::Invalid)?,
            attempts: Vec::new(),
            response: None,
            created_at: write_now.max(locked_now),
            updated_at: write_now.max(locked_now),
        };
        validate_record(&record)?;
        let row = seal_record_with_keys(&self.keys, &mut record, tag, entry_id)?;
        ensure_response_headroom(&record)?;
        insert_row(&transaction, &row)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        validate_admission_current(admission, invocation, AdmissionUse::Write, now)?;
        transaction.commit().map_err(map_sql_error)?;
        validate_admission_current(admission, invocation, AdmissionUse::Write, now)?;
        Ok(ReservationOutcome::Ready(ExactReservation {
            tag,
            entry_id,
            family: locator.family,
        }))
    }

    /// Atomically records the current grant/reference snapshot and consumes the only send permit.
    /// A dropped permit leaves `WriteClaimed`; reopening can only lead to exact receipt lookup.
    pub(crate) fn claim_send(
        &mut self,
        reservation: ExactReservation,
        admission: &TrustedInvocationAdmission,
        now: u64,
    ) -> Result<OneUseSendPermit, StoreError> {
        validate_admission_current(admission, &admission.invocation, AdmissionUse::Write, now)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        let claim_now =
            validate_admission_current(admission, &admission.invocation, AdmissionUse::Write, now)?;
        check_write_headroom(&transaction, &self.database_path)?;
        let row = read_row(&transaction, &reservation.tag)?.ok_or(StoreError::StoreCorrupt)?;
        if row.entry_id != reservation.entry_id {
            return Err(StoreError::StoreCorrupt);
        }
        let mut record = open_record_with_keys(&self.keys, &row)?;
        check_exact_match(&record, &admission.invocation, admission)?;
        if !matches!(record.state, EntryState::Prepared) {
            return match record.state {
                EntryState::Completed => Err(StoreError::Conflict),
                EntryState::WriteClaimed | EntryState::LookupClaimed | EntryState::Unknown => {
                    Err(StoreError::RecoveryRequired)
                }
                EntryState::DefinitelyNotSent => Err(StoreError::RecoveryRequired),
                EntryState::Prepared => Err(StoreError::StoreCorrupt),
            };
        }
        push_attempt(
            &mut record,
            admission.attempt(super::record::AttemptKind::Write),
        )?;
        ensure_response_headroom(&record)?;
        advance(&mut record, EntryState::WriteClaimed, claim_now)?;
        let next = seal_record_with_keys(&self.keys, &mut record, row.tag, row.entry_id)?;
        update_row(&transaction, &next)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        validate_admission_current(admission, &admission.invocation, AdmissionUse::Write, now)?;
        transaction.commit().map_err(map_sql_error)?;
        validate_admission_current(admission, &admission.invocation, AdmissionUse::Write, now)?;
        Ok(OneUseSendPermit {
            tag: row.tag,
            entry_id: row.entry_id,
            sequence: record.sequence,
            invocation: admission.invocation.clone(),
            endpoint: record.endpoint,
            body: Zeroizing::new(record.canonical_body.into_bytes()),
            reference: admission.reference.clone(),
        })
    }

    /// Claim only the operation-specific exact receipt endpoint. This method never manufactures a
    /// POST/PATCH/PUT retry; source uploads and adoption fail closed because their exact receipt
    /// contract is not pinned.
    pub(crate) fn claim_exact_lookup(
        &mut self,
        reservation: ExactReservation,
        current_invocation: &ContextOwnerInvocationV2,
        admission: &TrustedInvocationAdmission,
        now: u64,
    ) -> Result<OneUseLookupPermit, StoreError> {
        validate_admission_current(
            admission,
            current_invocation,
            AdmissionUse::ExactLookup,
            now,
        )?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sql_error)?;
        let claim_now = validate_admission_current(
            admission,
            current_invocation,
            AdmissionUse::ExactLookup,
            now,
        )?;
        check_write_headroom(&transaction, &self.database_path)?;
        let row = read_row(&transaction, &reservation.tag)?.ok_or(StoreError::StoreCorrupt)?;
        if row.entry_id != reservation.entry_id {
            return Err(StoreError::StoreCorrupt);
        }
        let mut record = open_record_with_keys(&self.keys, &row)?;
        check_exact_match(&record, current_invocation, admission)?;
        if !reservation.family.has_exact_lookup() {
            return Err(StoreError::AmbiguousWithoutExactLookup);
        }
        if !matches!(
            record.state,
            EntryState::WriteClaimed | EntryState::LookupClaimed | EntryState::Unknown
        ) {
            return Err(StoreError::RecoveryRequired);
        }
        let lookup = lookup_invocation(&record.invocation, current_invocation)
            .map_err(|_| StoreError::Denied)?;
        let endpoint = lookup.endpoint().map_err(|_| StoreError::Invalid)?;
        let body = lookup
            .harness_body()
            .map_err(|_| StoreError::Invalid)?
            .ok_or(StoreError::Invalid)?;
        if body.len() > MAX_HARNESS_JSON_BODY_BYTES {
            return Err(StoreError::StorageLimit);
        }
        push_attempt(
            &mut record,
            admission.attempt(super::record::AttemptKind::Lookup),
        )?;
        ensure_response_headroom(&record)?;
        advance(&mut record, EntryState::LookupClaimed, claim_now)?;
        let next = seal_record_with_keys(&self.keys, &mut record, row.tag, row.entry_id)?;
        update_row(&transaction, &next)?;
        check_storage_bounds(&transaction, &self.database_path)?;
        validate_admission_current(
            admission,
            current_invocation,
            AdmissionUse::ExactLookup,
            now,
        )?;
        transaction.commit().map_err(map_sql_error)?;
        validate_admission_current(
            admission,
            current_invocation,
            AdmissionUse::ExactLookup,
            now,
        )?;
        Ok(OneUseLookupPermit {
            tag: row.tag,
            entry_id: row.entry_id,
            sequence: record.sequence,
            original: current_invocation.clone(),
            invocation: lookup,
            endpoint,
            body: Zeroizing::new(body),
            reference: admission.reference.clone(),
        })
    }

    pub(crate) fn check_storage(&self) -> Result<(), StoreError> {
        check_storage_bounds(&self.connection, &self.database_path)
    }

    pub(super) fn validate_all_rows(&mut self) -> Result<(), StoreError> {
        validate_schema(&self.connection)?;
        let tags = {
            let mut statement = self
                .connection
                .prepare("SELECT lookup_tag FROM owner_invocations ORDER BY lookup_tag")
                .map_err(|_| StoreError::StoreUnavailable)?;
            statement
                .query_map([], |row| row.get::<_, Vec<u8>>(0))
                .map_err(|_| StoreError::StoreUnavailable)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| StoreError::StoreCorrupt)?
        };
        for tag in tags {
            let tag: [u8; 32] = tag.try_into().map_err(|_| StoreError::StoreCorrupt)?;
            let row = read_row(&self.connection, &tag)?.ok_or(StoreError::StoreCorrupt)?;
            open_record_with_keys(&self.keys, &row)?;
        }
        Ok(())
    }
}

impl<K: OwnerInvocationKeyProvider> std::fmt::Debug for OwnerInvocationStore<K> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OwnerInvocationStore(<encrypted>)")
    }
}

impl OneUseSendPermit {
    pub(crate) fn invocation(&self) -> &ContextOwnerInvocationV2 {
        &self.invocation
    }

    pub(crate) fn endpoint(&self) -> &ContextOwnerEndpointV1 {
        &self.endpoint
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn protected_reference(&self) -> &crate::harness_facade::ProtectedAuthReference {
        &self.reference
    }
}

impl OneUseLookupPermit {
    pub(crate) fn original_invocation(&self) -> &ContextOwnerInvocationV2 {
        &self.original
    }

    pub(crate) fn invocation(&self) -> &ContextOwnerInvocationV2 {
        &self.invocation
    }

    pub(crate) fn endpoint(&self) -> &ContextOwnerEndpointV1 {
        &self.endpoint
    }

    pub(crate) fn body(&self) -> &[u8] {
        &self.body
    }

    pub(crate) fn protected_reference(&self) -> &crate::harness_facade::ProtectedAuthReference {
        &self.reference
    }
}
