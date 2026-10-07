#[cfg(unix)]
use rusqlite::{Connection, OpenFlags};
use std::ffi::OsString;
#[cfg(unix)]
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use super::super::StoreError;
#[cfg(unix)]
use super::core::{
    MAX_AUXILIARY_BYTES, MAX_DATABASE_BYTES, MAX_DATABASE_PAGES, MAX_WAL_BYTES, REQUIRED_PAGE_SIZE,
    check_storage_bounds,
};
#[cfg(unix)]
use super::schema::validate_schema;

#[cfg(unix)]
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(unix)]
const JOURNAL_SIZE_LIMIT: i64 = 8 * 1024 * 1024;
pub(in crate::owner_invocation_store) fn open_database(
    path: &Path,
) -> Result<(rusqlite::Connection, PathBuf), StoreError> {
    #[cfg(unix)]
    {
        open_database_unix(path)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(StoreError::UnsupportedPlatform)
    }
}

#[cfg(unix)]
fn open_database_unix(path: &Path) -> Result<(Connection, PathBuf), StoreError> {
    let path = checked_database_path(path)?;
    check_preexisting_sidecars(&path)?;
    let connection = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|_| StoreError::StoreUnavailable)?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|_| StoreError::StoreUnavailable)?;
    check_storage_bounds(&connection, &path)?;
    let page_size: u32 = connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(|_| StoreError::StoreUnavailable)?;
    if page_size != REQUIRED_PAGE_SIZE {
        return Err(StoreError::StoreCorrupt);
    }
    let exists = validate_schema(&connection)?;
    if !exists {
        connection
            .pragma_update(None, "page_size", REQUIRED_PAGE_SIZE)
            .map_err(|_| StoreError::StoreUnavailable)?;
    }
    connection
        .pragma_update(None, "max_page_count", MAX_DATABASE_PAGES)
        .map_err(|_| StoreError::StoreUnavailable)?;
    let pages: u32 = connection
        .query_row("PRAGMA max_page_count", [], |row| row.get(0))
        .map_err(|_| StoreError::StoreUnavailable)?;
    if pages != MAX_DATABASE_PAGES {
        return Err(StoreError::StorageLimit);
    }
    let journal: String = connection
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|_| StoreError::StoreUnavailable)?;
    if !journal.eq_ignore_ascii_case("wal") {
        return Err(StoreError::StoreUnavailable);
    }
    connection
        .pragma_update(None, "foreign_keys", true)
        .and_then(|()| connection.pragma_update(None, "synchronous", "FULL"))
        .and_then(|()| connection.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT))
        .map_err(|_| StoreError::StoreUnavailable)?;
    let limit: i64 = connection
        .query_row("PRAGMA journal_size_limit", [], |row| row.get(0))
        .map_err(|_| StoreError::StoreUnavailable)?;
    if limit != JOURNAL_SIZE_LIMIT {
        return Err(StoreError::StoreUnavailable);
    }
    check_storage_bounds(&connection, &path)?;
    if !exists {
        let transaction = connection
            .unchecked_transaction()
            .map_err(|_| StoreError::StoreUnavailable)?;
        transaction
            .execute_batch(CREATE_SCHEMA_SQL)
            .map_err(|_| StoreError::StoreUnavailable)?;
        transaction
            .commit()
            .map_err(|_| StoreError::StoreUnavailable)?;
    }
    Ok((connection, path))
}

#[cfg(unix)]
fn checked_database_path(path: &Path) -> Result<PathBuf, StoreError> {
    let filename = path.file_name().ok_or(StoreError::Invalid)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    reject_symlink_directory_components(parent)?;
    let canonical_parent = fs::canonicalize(parent).map_err(|_| StoreError::StoreUnavailable)?;
    let metadata = fs::metadata(&canonical_parent).map_err(|_| StoreError::StoreUnavailable)?;
    if !metadata.is_dir() {
        return Err(StoreError::Invalid);
    }
    verify_private_directory(&metadata)?;
    let canonical_path = canonical_parent.join(filename);
    match fs::symlink_metadata(&canonical_path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(StoreError::Invalid);
        }
        Ok(metadata) => verify_private_file(&metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_private_file(&canonical_path)?;
        }
        Err(_) => return Err(StoreError::StoreUnavailable),
    }
    let metadata =
        fs::symlink_metadata(&canonical_path).map_err(|_| StoreError::StoreUnavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(StoreError::Invalid);
    }
    verify_private_file(&metadata)?;
    Ok(canonical_path)
}

#[cfg(unix)]
fn check_preexisting_sidecars(path: &Path) -> Result<(), StoreError> {
    bounded_size(&with_suffix(path, "-wal"), MAX_WAL_BYTES)?;
    bounded_size(&with_suffix(path, "-shm"), MAX_AUXILIARY_BYTES)?;
    bounded_size(&with_suffix(path, "-journal"), MAX_AUXILIARY_BYTES)?;
    Ok(())
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<(), StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    match options.open(path) {
        Ok(file) => drop(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(StoreError::StoreUnavailable),
    }
    Ok(())
}

#[cfg(unix)]
fn verify_private_directory(metadata: &fs::Metadata) -> Result<(), StoreError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(StoreError::Invalid);
    }
    Ok(())
}

#[cfg(unix)]
fn verify_private_file(metadata: &fs::Metadata) -> Result<(), StoreError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    verify_private_file_attributes(
        metadata.uid(),
        metadata.nlink(),
        metadata.permissions().mode(),
    )?;
    Ok(())
}

#[cfg(unix)]
fn verify_private_file_attributes(uid: u32, links: u64, mode: u32) -> Result<(), StoreError> {
    if uid != rustix::process::geteuid().as_raw() || links != 1 || mode & 0o077 != 0 {
        return Err(StoreError::Invalid);
    }
    Ok(())
}

#[cfg(unix)]
fn reject_symlink_directory_components(path: &Path) -> Result<(), StoreError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| StoreError::StoreUnavailable)?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|_| StoreError::StoreUnavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(StoreError::Invalid);
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn bounded_size(path: &Path, max_bytes: u64) -> Result<u64, StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(StoreError::StoreUnavailable)
        }
        Ok(metadata) if metadata.len() > max_bytes => Err(StoreError::StorageLimit),
        Ok(metadata) => {
            verify_private_file(&metadata)?;
            Ok(metadata.len())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => Err(StoreError::StoreUnavailable),
    }
}

#[cfg(not(unix))]
pub(super) fn bounded_size(_path: &Path, _max_bytes: u64) -> Result<u64, StoreError> {
    Err(StoreError::UnsupportedPlatform)
}

pub(super) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{StoreError, verify_private_file_attributes};

    #[test]
    fn private_file_policy_rejects_foreign_owner_and_multiple_links() {
        let owner = rustix::process::geteuid().as_raw();
        let foreign_owner = if owner == 0 { 1 } else { 0 };
        assert_eq!(
            verify_private_file_attributes(foreign_owner, 1, 0o600),
            Err(StoreError::Invalid)
        );
        assert_eq!(
            verify_private_file_attributes(owner, 2, 0o600),
            Err(StoreError::Invalid)
        );
    }
}

#[cfg(unix)]
pub(super) const CREATE_SCHEMA_SQL: &str = "CREATE TABLE owner_invocation_meta (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version = 2),
    index_key_id TEXT NOT NULL CHECK (length(CAST(index_key_id AS BLOB)) BETWEEN 1 AND 64),
    index_key_check BLOB NOT NULL CHECK (length(index_key_check) = 32)
);
CREATE TABLE owner_invocations (
    lookup_tag BLOB PRIMARY KEY NOT NULL CHECK (length(lookup_tag) = 32),
    entry_id BLOB UNIQUE NOT NULL CHECK (length(entry_id) = 16),
    state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 5),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    data_key_id TEXT NOT NULL CHECK (length(CAST(data_key_id AS BLOB)) BETWEEN 1 AND 64),
    nonce BLOB NOT NULL CHECK (length(nonce) = 24),
    ciphertext BLOB NOT NULL CHECK (length(ciphertext) BETWEEN 16 AND 6291456)
);";
