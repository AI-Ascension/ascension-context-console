// SPDX-License-Identifier: MIT

use super::{AdmittedSubjectGrant, SubjectGrantError};
use crate::control::Scope;
use crate::harness_facade::FacadePermission;
use rusqlite::{Connection, params};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

mod schema;
pub(super) use schema::{create_schema, validate_existing_schema};

pub(super) const MAX_DATABASE_PAGES: u32 = 16_384;
pub(super) const MAX_DATABASE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_WAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_AUXILIARY_BYTES: u64 = 1024 * 1024;
pub(super) const REQUIRED_PAGE_SIZE: u32 = 4 * 1024;
pub(super) const MAX_JOURNAL_SIZE_BYTES: i64 = 8 * 1024 * 1024;
// Every variable text field is capped at 128 bytes and permission names are fixed below 32
// bytes, so a table row and each index key stay below 1 KiB. With 4 KiB pages and at most
// 16,384 pages, the table, primary-key index and lookup index each have at most eight levels
// (a 4-way minimum fanout already exceeds the page ceiling at eight). A split at every level
// touches at most 17 WAL frames per tree; even 64 frames including commit/header overhead are
// below 264 KiB. The 1 MiB reserve also covers a checkpoint copy. Revocation dirties one table
// leaf. Writes fail closed before starting without this fixed-schema reserve.
pub(super) const MAX_SINGLE_WRITE_GROWTH_BYTES: u64 = 1024 * 1024;
const MAX_TOTAL_STORE_BYTES: u64 = MAX_DATABASE_BYTES + MAX_WAL_BYTES + 2 * MAX_AUXILIARY_BYTES;

pub(super) fn find_admitted_grant(
    transaction: &rusqlite::Transaction<'_>,
    issuer: &str,
    subject: &str,
    scope: &Scope,
    permission: FacadePermission,
    now: u64,
) -> Result<Option<AdmittedSubjectGrant>, SubjectGrantError> {
    let mut statement = transaction
        .prepare(
            "SELECT grant_id, issuer, subject, not_before, expires_at,
                    revocation_generation, revoked
             FROM console_subject_grants
             WHERE issuer = ?1 AND subject = ?2 AND project_id = ?3 AND run_id = ?4
               AND episode_id = ?5 AND agent_id = ?6 AND permission = ?7
             ORDER BY expires_at DESC, grant_id ASC",
        )
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    let mut rows = statement
        .query(params![
            issuer,
            subject,
            scope.project_id,
            scope.run_id,
            scope.episode_id,
            scope.agent_id,
            permission.as_str(),
        ])
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    while let Some(row) = rows
        .next()
        .map_err(|_| SubjectGrantError::StoreUnavailable)?
    {
        let decoded = (|| {
            Ok::<_, rusqlite::Error>((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })()
        .map_err(|_| SubjectGrantError::Corrupt)?;
        let (grant_id, stored_issuer, stored_subject, not_before, expires_at, generation, revoked) =
            decoded;
        let not_before = u64::try_from(not_before).map_err(|_| SubjectGrantError::Corrupt)?;
        let expires_at = u64::try_from(expires_at).map_err(|_| SubjectGrantError::Corrupt)?;
        let revocation_generation =
            u64::try_from(generation).map_err(|_| SubjectGrantError::Corrupt)?;
        if expires_at <= not_before || !matches!(revoked, 0 | 1) {
            return Err(SubjectGrantError::Corrupt);
        }
        if revoked == 0 && not_before <= now && expires_at > now {
            return Ok(Some(AdmittedSubjectGrant {
                grant_id,
                issuer: stored_issuer,
                subject: stored_subject,
                permission,
                scope: scope.clone(),
                not_before,
                expires_at,
                revocation_generation,
            }));
        }
    }
    Ok(None)
}

pub(super) fn checked_database_path(path: &Path) -> Result<PathBuf, SubjectGrantError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        return Err(SubjectGrantError::Invalid);
    }
    let connection = Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE,
    )
    .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    drop(connection);
    fs::canonicalize(path).map_err(|_| SubjectGrantError::StoreUnavailable)
}

pub(super) fn check_connection_storage_bounds(
    connection: &Connection,
    database_path: &Path,
) -> Result<(), SubjectGrantError> {
    let pages: u32 = connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    if pages > MAX_DATABASE_PAGES {
        return Err(SubjectGrantError::StorageLimit);
    }
    let wal = with_suffix(database_path, "-wal");
    let shm = with_suffix(database_path, "-shm");
    let rollback_journal = with_suffix(database_path, "-journal");
    let database_bytes = bounded_file_size(database_path, MAX_DATABASE_BYTES)?;
    let wal_bytes = bounded_file_size(&wal, MAX_WAL_BYTES)?;
    let shm_bytes = bounded_file_size(&shm, MAX_AUXILIARY_BYTES)?;
    let journal_bytes = bounded_file_size(&rollback_journal, MAX_AUXILIARY_BYTES)?;
    let total = database_bytes
        .checked_add(wal_bytes)
        .and_then(|size| size.checked_add(shm_bytes))
        .and_then(|size| size.checked_add(journal_bytes))
        .ok_or(SubjectGrantError::StorageLimit)?;
    if total > MAX_TOTAL_STORE_BYTES {
        return Err(SubjectGrantError::StorageLimit);
    }
    Ok(())
}

pub(super) fn check_write_headroom(
    connection: &Connection,
    database_path: &Path,
) -> Result<(), SubjectGrantError> {
    check_connection_storage_bounds(connection, database_path)?;
    let wal = with_suffix(database_path, "-wal");
    let database_bytes = bounded_file_size(database_path, MAX_DATABASE_BYTES)?;
    let wal_bytes = bounded_file_size(&wal, MAX_WAL_BYTES)?;
    if database_bytes > MAX_DATABASE_BYTES.saturating_sub(MAX_SINGLE_WRITE_GROWTH_BYTES)
        || wal_bytes > MAX_WAL_BYTES.saturating_sub(MAX_SINGLE_WRITE_GROWTH_BYTES)
    {
        return Err(SubjectGrantError::StorageLimit);
    }
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

fn bounded_file_size(path: &Path, maximum: u64) -> Result<u64, SubjectGrantError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(SubjectGrantError::StoreUnavailable)
        }
        Ok(metadata) if metadata.len() > maximum => Err(SubjectGrantError::StorageLimit),
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => Err(SubjectGrantError::StoreUnavailable),
    }
}

#[cfg(test)]
mod tests;
