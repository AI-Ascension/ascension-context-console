use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::path::Path;

use super::super::StoreError;
use super::super::record::EntryState;
use super::open::{bounded_size, with_suffix};

pub(super) const REQUIRED_PAGE_SIZE: u32 = 4096;
pub(super) const MAX_DATABASE_PAGES: u32 = 16_384;
pub(super) const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_WAL_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const MAX_AUXILIARY_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_TOTAL_STORE_BYTES: u64 =
    MAX_DATABASE_BYTES + MAX_WAL_BYTES + 2 * MAX_AUXILIARY_BYTES;
const MAX_SINGLE_WRITE_GROWTH_BYTES: u64 = 8 * 1024 * 1024;
pub(super) const MAX_CIPHERTEXT_BYTES: usize = 6 * 1024 * 1024;
pub(super) const MAX_ROWS: usize = 16;
pub(super) struct StoredRow {
    pub tag: [u8; 32],
    pub entry_id: [u8; 16],
    pub state: EntryState,
    pub sequence: u64,
    pub data_key_id: String,
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}
pub(super) fn row_count(connection: &Connection) -> Result<usize, StoreError> {
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM owner_invocations", [], |row| {
            row.get(0)
        })
        .map_err(|_| StoreError::StoreCorrupt)?;
    usize::try_from(count).map_err(|_| StoreError::StoreCorrupt)
}

pub(super) fn read_row(
    connection: &Connection,
    lookup_tag: &[u8; 32],
) -> Result<Option<StoredRow>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT length(lookup_tag), length(entry_id), state, sequence, typeof(data_key_id),
                    length(CAST(data_key_id AS BLOB)), length(nonce), length(ciphertext)
             FROM owner_invocations WHERE lookup_tag = ?1",
        )
        .map_err(|_| StoreError::StoreUnavailable)?;
    let Some((
        tag_len,
        id_len,
        state,
        sequence,
        key_id_type,
        key_id_len,
        nonce_len,
        ciphertext_len,
    )) = statement
        .query_row([lookup_tag], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .optional()
        .map_err(|_| StoreError::StoreCorrupt)?
    else {
        return Ok(None);
    };
    if tag_len != 32
        || id_len != 16
        || EntryState::from_sql(state).is_none()
        || sequence < 1
        || key_id_type != "text"
        || !(1..=64).contains(&key_id_len)
        || nonce_len != 24
        || !(16..=MAX_CIPHERTEXT_BYTES as i64).contains(&ciphertext_len)
    {
        return Err(StoreError::StoreCorrupt);
    }
    let row = connection
        .query_row(
            "SELECT lookup_tag, entry_id, state, sequence, data_key_id, nonce, ciphertext
             FROM owner_invocations WHERE lookup_tag = ?1",
            [lookup_tag],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            },
        )
        .map_err(|_| StoreError::StoreCorrupt)?;
    let tag: [u8; 32] = row.0.try_into().map_err(|_| StoreError::StoreCorrupt)?;
    let entry_id: [u8; 16] = row.1.try_into().map_err(|_| StoreError::StoreCorrupt)?;
    let state = EntryState::from_sql(row.2).ok_or(StoreError::StoreCorrupt)?;
    let sequence = u64::try_from(row.3).map_err(|_| StoreError::StoreCorrupt)?;
    let nonce: [u8; 24] = row.5.try_into().map_err(|_| StoreError::StoreCorrupt)?;
    Ok(Some(StoredRow {
        tag,
        entry_id,
        state,
        sequence,
        data_key_id: row.4,
        nonce,
        ciphertext: row.6,
    }))
}

pub(super) fn insert_row(transaction: &Transaction<'_>, row: &StoredRow) -> Result<(), StoreError> {
    transaction
        .execute(
            "INSERT INTO owner_invocations
                (lookup_tag, entry_id, state, sequence, data_key_id, nonce, ciphertext)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.tag.as_slice(),
                row.entry_id.as_slice(),
                row.state as i64,
                i64::try_from(row.sequence).map_err(|_| StoreError::StorageLimit)?,
                row.data_key_id,
                row.nonce.as_slice(),
                row.ciphertext,
            ],
        )
        .map_err(map_sql_error)?;
    Ok(())
}

pub(super) fn update_row(transaction: &Transaction<'_>, row: &StoredRow) -> Result<(), StoreError> {
    let changed = transaction
        .execute(
            "UPDATE owner_invocations SET state = ?1, sequence = ?2, data_key_id = ?3,
                    nonce = ?4, ciphertext = ?5 WHERE lookup_tag = ?6 AND entry_id = ?7",
            params![
                row.state as i64,
                i64::try_from(row.sequence).map_err(|_| StoreError::StorageLimit)?,
                row.data_key_id,
                row.nonce.as_slice(),
                row.ciphertext,
                row.tag.as_slice(),
                row.entry_id.as_slice(),
            ],
        )
        .map_err(map_sql_error)?;
    if changed != 1 {
        return Err(StoreError::StoreCorrupt);
    }
    Ok(())
}

pub(super) fn check_storage_bounds(
    connection: &Connection,
    database_path: &Path,
) -> Result<(), StoreError> {
    let pages: u32 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(|_| StoreError::StoreUnavailable)?;
    if pages > MAX_DATABASE_PAGES {
        return Err(StoreError::StorageLimit);
    }
    let database_bytes = bounded_size(database_path, MAX_DATABASE_BYTES)?;
    let wal_bytes = bounded_size(&with_suffix(database_path, "-wal"), MAX_WAL_BYTES)?;
    let shm_bytes = bounded_size(&with_suffix(database_path, "-shm"), MAX_AUXILIARY_BYTES)?;
    let journal_bytes = bounded_size(&with_suffix(database_path, "-journal"), MAX_AUXILIARY_BYTES)?;
    let total = database_bytes
        .checked_add(wal_bytes)
        .and_then(|bytes| bytes.checked_add(shm_bytes))
        .and_then(|bytes| bytes.checked_add(journal_bytes))
        .ok_or(StoreError::StorageLimit)?;
    if total > MAX_TOTAL_STORE_BYTES {
        return Err(StoreError::StorageLimit);
    }
    Ok(())
}

pub(super) fn check_write_headroom(connection: &Connection, path: &Path) -> Result<(), StoreError> {
    check_storage_bounds(connection, path)?;
    let database = bounded_size(path, MAX_DATABASE_BYTES)?;
    let wal = bounded_size(&with_suffix(path, "-wal"), MAX_WAL_BYTES)?;
    if database > MAX_DATABASE_BYTES.saturating_sub(MAX_SINGLE_WRITE_GROWTH_BYTES)
        || wal > MAX_WAL_BYTES.saturating_sub(MAX_SINGLE_WRITE_GROWTH_BYTES)
    {
        return Err(StoreError::StorageLimit);
    }
    Ok(())
}

pub(super) fn map_sql_error(error: rusqlite::Error) -> StoreError {
    match error {
        rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DiskFull => {
            StoreError::StorageLimit
        }
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::DatabaseBusy =>
        {
            StoreError::StoreUnavailable
        }
        _ => StoreError::StoreUnavailable,
    }
}
