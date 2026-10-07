use rusqlite::{Connection, params};
use std::path::Path;

use super::super::StoreError;
use super::super::crypto::{OwnerInvocationKeyMaterial, index_key_verifier, verify_index_key};
use super::core::{
    MAX_CIPHERTEXT_BYTES, MAX_ROWS, check_storage_bounds, check_write_headroom, map_sql_error,
};
pub(super) fn initialize_index_id(
    connection: &mut Connection,
    path: &Path,
    keys: &OwnerInvocationKeyMaterial,
) -> Result<(), StoreError> {
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(map_sql_error)?;
    check_write_headroom(&transaction, path)?;
    let count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM owner_invocation_meta", [], |row| {
            row.get(0)
        })
        .map_err(|_| StoreError::StoreCorrupt)?;
    match count {
        0 => {
            let rows: i64 = transaction
                .query_row("SELECT COUNT(*) FROM owner_invocations", [], |row| {
                    row.get(0)
                })
                .map_err(|_| StoreError::StoreCorrupt)?;
            if rows != 0 {
                return Err(StoreError::StoreCorrupt);
            }
            let verifier = index_key_verifier(keys.index_key(), keys.index_key_id())?;
            transaction
                .execute(
                    "INSERT INTO owner_invocation_meta
                     (singleton, schema_version, index_key_id, index_key_check)
                     VALUES (1, 2, ?1, ?2)",
                    params![keys.index_key_id(), verifier.as_slice()],
                )
                .map_err(map_sql_error)?;
        }
        1 => {
            let (version, key_type, key_len, check_type, check_len):
                (i64, String, i64, String, i64) = transaction
                .query_row(
                    "SELECT schema_version, typeof(index_key_id), length(CAST(index_key_id AS BLOB)),
                            typeof(index_key_check), length(index_key_check)
                     FROM owner_invocation_meta WHERE singleton = 1",
                    [],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                        ))
                    },
                )
                .map_err(|_| StoreError::StoreCorrupt)?;
            if version != 2
                || key_type != "text"
                || !(1..=64).contains(&key_len)
                || check_type != "blob"
                || check_len != 32
            {
                return Err(StoreError::StoreCorrupt);
            }
            let (version, index_key_id, verifier): (i64, String, Vec<u8>) = transaction
                .query_row(
                    "SELECT schema_version, index_key_id, index_key_check
                     FROM owner_invocation_meta WHERE singleton = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| StoreError::StoreCorrupt)?;
            if version != 2 || index_key_id != keys.index_key_id() {
                return Err(StoreError::IndexKeyRotationRequiresOfflineMigration);
            }
            verify_index_key(keys.index_key(), &index_key_id, &verifier)?;
        }
        _ => return Err(StoreError::StoreCorrupt),
    }
    check_storage_bounds(&transaction, path)?;
    transaction.commit().map_err(map_sql_error)
}

pub(super) fn validate_schema(connection: &Connection) -> Result<bool, StoreError> {
    let mut statement = connection
        .prepare("SELECT name, sql FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .map_err(|_| StoreError::StoreUnavailable)?;
    let tables = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| StoreError::StoreUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StoreError::StoreCorrupt)?;
    if tables.is_empty() {
        let objects: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| StoreError::StoreUnavailable)?;
        return if objects == 0 {
            Ok(false)
        } else {
            Err(StoreError::StoreCorrupt)
        };
    }
    if tables.len() != 2
        || tables[0].0 != "owner_invocation_meta"
        || compact_sql(&tables[0].1) != compact_sql(META_TABLE_SQL)
        || tables[1].0 != "owner_invocations"
        || compact_sql(&tables[1].1) != compact_sql(INVOCATION_TABLE_SQL)
    {
        return Err(StoreError::StoreCorrupt);
    }
    let extra_objects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type NOT IN ('table', 'index') AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::StoreUnavailable)?;
    if extra_objects != 0 {
        return Err(StoreError::StoreCorrupt);
    }
    let unexpected_indexes: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'index' AND name NOT LIKE 'sqlite_autoindex_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::StoreUnavailable)?;
    let automatic_indexes: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'index' AND name LIKE 'sqlite_autoindex_%'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::StoreUnavailable)?;
    if unexpected_indexes != 0 || automatic_indexes != 2 {
        return Err(StoreError::StoreCorrupt);
    }
    validate_columns(
        connection,
        "owner_invocation_meta",
        &[
            "singleton",
            "schema_version",
            "index_key_id",
            "index_key_check",
        ],
    )?;
    validate_columns(
        connection,
        "owner_invocations",
        &[
            "lookup_tag",
            "entry_id",
            "state",
            "sequence",
            "data_key_id",
            "nonce",
            "ciphertext",
        ],
    )?;
    validate_rows(connection)?;
    Ok(true)
}

pub(super) fn validate_rows(connection: &Connection) -> Result<(), StoreError> {
    let invalid: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM owner_invocations WHERE
                typeof(lookup_tag) != 'blob' OR length(lookup_tag) != 32 OR
                typeof(entry_id) != 'blob' OR length(entry_id) != 16 OR
                typeof(state) != 'integer' OR state NOT BETWEEN 0 AND 5 OR
                typeof(sequence) != 'integer' OR sequence < 1 OR
                typeof(data_key_id) != 'text' OR length(CAST(data_key_id AS BLOB)) NOT BETWEEN 1 AND 64 OR
                typeof(nonce) != 'blob' OR length(nonce) != 24 OR
                typeof(ciphertext) != 'blob' OR length(ciphertext) NOT BETWEEN 16 AND ?1",
            params![MAX_CIPHERTEXT_BYTES as i64],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::StoreCorrupt)?;
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM owner_invocations", [], |row| {
            row.get(0)
        })
        .map_err(|_| StoreError::StoreCorrupt)?;
    if invalid != 0 || count < 0 || count > MAX_ROWS as i64 {
        return Err(StoreError::StoreCorrupt);
    }
    let metadata_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM owner_invocation_meta", [], |row| {
            row.get(0)
        })
        .map_err(|_| StoreError::StoreCorrupt)?;
    if metadata_count > 1 {
        return Err(StoreError::StoreCorrupt);
    }
    let invalid_metadata: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM owner_invocation_meta WHERE
                typeof(singleton) != 'integer' OR singleton != 1 OR
                typeof(schema_version) != 'integer' OR schema_version != 2 OR
                typeof(index_key_id) != 'text' OR length(CAST(index_key_id AS BLOB)) NOT BETWEEN 1 AND 64 OR
                typeof(index_key_check) != 'blob' OR length(index_key_check) != 32",
            [],
            |row| row.get(0),
        )
        .map_err(|_| StoreError::StoreCorrupt)?;
    if invalid_metadata != 0 {
        return Err(StoreError::StoreCorrupt);
    }
    Ok(())
}

fn validate_columns(
    connection: &Connection,
    table: &str,
    expected: &[&str],
) -> Result<(), StoreError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_xinfo({table})"))
        .map_err(|_| StoreError::StoreUnavailable)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|_| StoreError::StoreUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| StoreError::StoreCorrupt)?;
    if columns.len() != expected.len()
        || columns
            .iter()
            .zip(expected)
            .any(|(actual, required)| actual != required)
    {
        return Err(StoreError::StoreCorrupt);
    }
    Ok(())
}

fn compact_sql(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

const META_TABLE_SQL: &str = "CREATE TABLE owner_invocation_meta (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version = 2),
    index_key_id TEXT NOT NULL CHECK (length(CAST(index_key_id AS BLOB)) BETWEEN 1 AND 64),
    index_key_check BLOB NOT NULL CHECK (length(index_key_check) = 32)
)";
const INVOCATION_TABLE_SQL: &str = "CREATE TABLE owner_invocations (
    lookup_tag BLOB PRIMARY KEY NOT NULL CHECK (length(lookup_tag) = 32),
    entry_id BLOB UNIQUE NOT NULL CHECK (length(entry_id) = 16),
    state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 5),
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    data_key_id TEXT NOT NULL CHECK (length(CAST(data_key_id AS BLOB)) BETWEEN 1 AND 64),
    nonce BLOB NOT NULL CHECK (length(nonce) = 24),
    ciphertext BLOB NOT NULL CHECK (length(ciphertext) BETWEEN 16 AND 6291456)
)";
