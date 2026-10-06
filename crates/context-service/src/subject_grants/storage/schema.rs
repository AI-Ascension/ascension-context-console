// SPDX-License-Identifier: MIT

use super::super::{MAX_SUBJECT_GRANTS, SubjectGrantError};
use rusqlite::{Connection, Transaction};

const CREATE_TABLE_SQL: &str = "CREATE TABLE console_subject_grants (
    grant_id TEXT PRIMARY KEY NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    permission TEXT NOT NULL,
    project_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    episode_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    not_before INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    revocation_generation INTEGER NOT NULL,
    revoked INTEGER NOT NULL CHECK (revoked IN (0, 1))
);";

const CREATE_INDEX_SQL: &str = "CREATE INDEX console_subject_grants_lookup
    ON console_subject_grants (
        issuer, subject, project_id, run_id, episode_id, agent_id, permission
    );";

const LOOKUP_COLUMNS: &[&str] = &[
    "issuer",
    "subject",
    "project_id",
    "run_id",
    "episode_id",
    "agent_id",
    "permission",
];

pub(in crate::subject_grants) fn validate_existing_schema(
    connection: &Connection,
) -> Result<bool, SubjectGrantError> {
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY type, name")
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    let objects = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|_| SubjectGrantError::StoreUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SubjectGrantError::Corrupt)?;
    if objects.is_empty() {
        return Ok(false);
    }
    if objects.len() != 3 {
        return Err(SubjectGrantError::Corrupt);
    }

    let table_sql = objects.iter().find_map(|(kind, name, table, sql)| {
        (kind == "table" && name == "console_subject_grants" && table == name)
            .then_some(sql.as_deref())
            .flatten()
    });
    let lookup_sql = objects.iter().find_map(|(kind, name, table, sql)| {
        (kind == "index"
            && name == "console_subject_grants_lookup"
            && table == "console_subject_grants")
            .then_some(sql.as_deref())
            .flatten()
    });
    let primary_index = objects.iter().any(|(kind, name, table, sql)| {
        kind == "index"
            && name == "sqlite_autoindex_console_subject_grants_1"
            && table == "console_subject_grants"
            && sql.is_none()
    });
    if !primary_index
        || !table_sql.is_some_and(|sql| compact_sql(sql) == compact_sql(CREATE_TABLE_SQL))
        || !lookup_sql.is_some_and(|sql| compact_sql(sql) == compact_sql(CREATE_INDEX_SQL))
    {
        return Err(SubjectGrantError::Corrupt);
    }

    validate_columns(connection)?;
    validate_indexes(connection)?;
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM console_subject_grants", [], |row| {
            row.get(0)
        })
        .map_err(|_| SubjectGrantError::Corrupt)?;
    if count < 0 || count > MAX_SUBJECT_GRANTS as i64 {
        return Err(SubjectGrantError::Corrupt);
    }
    validate_existing_rows(connection)?;
    Ok(true)
}

fn validate_existing_rows(connection: &Connection) -> Result<(), SubjectGrantError> {
    // Screen SQLite storage classes and byte lengths before decoding any attacker-controlled
    // TEXT into Rust strings. The validated table is capped at 4,096 rows, and every indexed
    // text key is then bounded to 128 bytes before it is materialized.
    let invalid_rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM console_subject_grants WHERE
                typeof(grant_id) != 'text' OR length(CAST(grant_id AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(issuer) != 'text' OR length(CAST(issuer AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(subject) != 'text' OR length(CAST(subject AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(permission) != 'text' OR length(CAST(permission AS BLOB)) NOT BETWEEN 1 AND 64 OR
                typeof(project_id) != 'text' OR length(CAST(project_id AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(run_id) != 'text' OR length(CAST(run_id AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(episode_id) != 'text' OR length(CAST(episode_id AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(agent_id) != 'text' OR length(CAST(agent_id AS BLOB)) NOT BETWEEN 1 AND 128 OR
                typeof(not_before) != 'integer' OR not_before < 0 OR
                typeof(expires_at) != 'integer' OR expires_at < 0 OR expires_at <= not_before OR
                typeof(revocation_generation) != 'integer' OR revocation_generation < 0 OR
                typeof(revoked) != 'integer' OR revoked NOT IN (0, 1) OR
                (revoked = 0 AND revocation_generation != 0) OR
                (revoked = 1 AND revocation_generation = 0) OR
                permission NOT IN (
                    'context.metadata.read', 'context.content.read', 'context.edit',
                    'context.objective.edit', 'context.commit', 'context.pause', 'context.resume'
                )",
            [],
            |row| row.get(0),
        )
        .map_err(|_| SubjectGrantError::Corrupt)?;
    if invalid_rows != 0 {
        return Err(SubjectGrantError::Corrupt);
    }

    let mut statement = connection
        .prepare(
            "SELECT grant_id, issuer, subject, project_id, run_id, episode_id, agent_id
             FROM console_subject_grants",
        )
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    for row in rows {
        let (grant_id, issuer, subject, project, run, episode, agent) =
            row.map_err(|_| SubjectGrantError::Corrupt)?;
        let scope = [project, run, episode, agent];
        if !crate::harness_facade::valid_id(&grant_id)
            || !super::super::valid_bounded_text(&issuer)
            || !crate::harness_facade::valid_id(&subject)
            || !scope
                .iter()
                .all(|value| crate::harness_facade::valid_id(value))
        {
            return Err(SubjectGrantError::Corrupt);
        }
    }
    Ok(())
}

pub(in crate::subject_grants) fn create_schema(
    transaction: &Transaction<'_>,
) -> Result<(), SubjectGrantError> {
    transaction
        .execute_batch(CREATE_TABLE_SQL)
        .and_then(|()| transaction.execute_batch(CREATE_INDEX_SQL))
        .map_err(|_| SubjectGrantError::StoreUnavailable)
}

fn validate_columns(connection: &Connection) -> Result<(), SubjectGrantError> {
    let expected = [
        ("grant_id", "TEXT", 1_i64),
        ("issuer", "TEXT", 0),
        ("subject", "TEXT", 0),
        ("permission", "TEXT", 0),
        ("project_id", "TEXT", 0),
        ("run_id", "TEXT", 0),
        ("episode_id", "TEXT", 0),
        ("agent_id", "TEXT", 0),
        ("not_before", "INTEGER", 0),
        ("expires_at", "INTEGER", 0),
        ("revocation_generation", "INTEGER", 0),
        ("revoked", "INTEGER", 0),
    ];
    let mut statement = connection
        .prepare("PRAGMA table_xinfo(console_subject_grants)")
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .map_err(|_| SubjectGrantError::StoreUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SubjectGrantError::Corrupt)?;
    if rows.len() != expected.len() {
        return Err(SubjectGrantError::Corrupt);
    }
    for (index, row) in rows.iter().enumerate() {
        let (name, data_type, primary_key) = expected[index];
        if row.0 != index as i64
            || row.1 != name
            || !row.2.eq_ignore_ascii_case(data_type)
            || row.3 != 1
            || row.4.is_some()
            || row.5 != primary_key
            || row.6 != 0
        {
            return Err(SubjectGrantError::Corrupt);
        }
    }
    Ok(())
}

fn validate_indexes(connection: &Connection) -> Result<(), SubjectGrantError> {
    let mut statement = connection
        .prepare("PRAGMA index_list(console_subject_grants)")
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    let indexes = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|_| SubjectGrantError::StoreUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SubjectGrantError::Corrupt)?;
    if indexes.len() != 2
        || !indexes.iter().any(|(name, unique, origin, partial)| {
            name == "console_subject_grants_lookup"
                && *unique == 0
                && origin == "c"
                && *partial == 0
        })
        || !indexes.iter().any(|(name, unique, origin, partial)| {
            name == "sqlite_autoindex_console_subject_grants_1"
                && *unique == 1
                && origin == "pk"
                && *partial == 0
        })
    {
        return Err(SubjectGrantError::Corrupt);
    }

    let mut statement = connection
        .prepare("PRAGMA index_xinfo(console_subject_grants_lookup)")
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
    let columns = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(|_| SubjectGrantError::StoreUnavailable)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SubjectGrantError::Corrupt)?;
    if columns.len() != LOOKUP_COLUMNS.len() + 1 {
        return Err(SubjectGrantError::Corrupt);
    }
    for (index, row) in columns.iter().take(LOOKUP_COLUMNS.len()).enumerate() {
        if row.0 != index as i64
            || row.1.as_deref() != Some(LOOKUP_COLUMNS[index])
            || row.2 != 0
            || row.3.as_deref() != Some("BINARY")
            || row.4 != 1
        {
            return Err(SubjectGrantError::Corrupt);
        }
    }
    let rowid = columns.last().ok_or(SubjectGrantError::Corrupt)?;
    if rowid.0 != LOOKUP_COLUMNS.len() as i64
        || rowid.1.is_some()
        || rowid.2 != 0
        || rowid
            .3
            .as_deref()
            .is_some_and(|collation| !collation.eq_ignore_ascii_case("BINARY"))
        || rowid.4 != 0
    {
        return Err(SubjectGrantError::Corrupt);
    }
    Ok(())
}

fn compact_sql(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != ';')
        .flat_map(char::to_lowercase)
        .collect()
}
