// SPDX-License-Identifier: MIT

//! Durable, subject-bound authorization grants for the trusted Console ingress.
//!
//! Grant provisioning is a privileged local operation. There is deliberately no HTTP method for
//! a bearer to issue or revoke its own grant, and no bearer credential is stored in this table.

use crate::authenticated_ingress::VerifiedPrincipal;
use crate::control::Scope;
use crate::harness_facade::FacadePermission;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod storage;
use storage::{
    MAX_DATABASE_PAGES, MAX_JOURNAL_SIZE_BYTES, REQUIRED_PAGE_SIZE,
    check_connection_storage_bounds, check_write_headroom, checked_database_path, create_schema,
    find_admitted_grant, validate_existing_schema,
};

pub const MAX_SUBJECT_GRANTS: usize = 4096;
const MAX_GRANT_TEXT_BYTES: usize = crate::harness_facade::MAX_FACADE_ID_BYTES;

/// Privileged provisioning input. It contains identity and policy metadata, never a bearer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubjectGrantSpec {
    pub grant_id: String,
    pub issuer: String,
    pub subject: String,
    pub permission: FacadePermission,
    pub scope: Scope,
    pub not_before: u64,
    pub expires_at: u64,
}

/// The exact stored grant admitted at the transactionally serialized dispatch boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedSubjectGrant {
    pub grant_id: String,
    pub issuer: String,
    pub subject: String,
    pub permission: FacadePermission,
    pub scope: Scope,
    pub not_before: u64,
    pub expires_at: u64,
    pub revocation_generation: u64,
}

/// Error values intentionally avoid returning database details or another subject's grant data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubjectGrantError {
    Invalid,
    Duplicate,
    Capacity,
    Denied,
    NotFound,
    Corrupt,
    StorageLimit,
    StoreUnavailable,
}

impl std::fmt::Display for SubjectGrantError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "subject grant is invalid",
            Self::Duplicate => "subject grant identity is already in use",
            Self::Capacity => "subject grant store is full",
            Self::Denied => "subject grant is unavailable",
            Self::NotFound => "subject grant is unavailable",
            Self::Corrupt => "subject grant store is invalid",
            Self::StorageLimit => "subject grant store reached its size limit",
            Self::StoreUnavailable => "subject grant store is unavailable",
        })
    }
}

impl std::error::Error for SubjectGrantError {}

/// Store boundary used by trusted ingress. Implementations must linearize reservation with
/// revocation; a revocation after a successful reservation does not cancel an admitted call.
pub trait SubjectGrantStore {
    fn reserve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
    ) -> Result<Vec<AdmittedSubjectGrant>, SubjectGrantError>;
}

/// SQLite-backed durable subject grant store.
pub struct SqliteSubjectGrantStore {
    connection: Connection,
    database_path: PathBuf,
}

impl SqliteSubjectGrantStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SubjectGrantError> {
        let path = checked_database_path(path.as_ref())?;
        let connection =
            Connection::open(&path).map_err(|_| SubjectGrantError::StoreUnavailable)?;
        Self::initialize(connection, path)
    }

    pub fn open_read_write(path: impl AsRef<Path>) -> Result<Self, SubjectGrantError> {
        let path = checked_database_path(path.as_ref())?;
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        Self::initialize(connection, path)
    }

    fn initialize(
        mut connection: Connection,
        database_path: PathBuf,
    ) -> Result<Self, SubjectGrantError> {
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        check_connection_storage_bounds(&connection, &database_path)?;
        let page_size: u32 = connection
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        if page_size != REQUIRED_PAGE_SIZE {
            return Err(SubjectGrantError::StoreUnavailable);
        }
        let schema_exists = validate_existing_schema(&connection)?;
        connection
            .pragma_update(None, "max_page_count", MAX_DATABASE_PAGES)
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        let max_pages: u32 = connection
            .query_row("PRAGMA max_page_count", [], |row| row.get(0))
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        if max_pages != MAX_DATABASE_PAGES {
            return Err(SubjectGrantError::StorageLimit);
        }
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        if !journal_mode.eq_ignore_ascii_case("wal") {
            return Err(SubjectGrantError::StoreUnavailable);
        }
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        connection
            .pragma_update(None, "journal_size_limit", MAX_JOURNAL_SIZE_BYTES)
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        let journal_size_limit: i64 = connection
            .query_row("PRAGMA journal_size_limit", [], |row| row.get(0))
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        if journal_size_limit != MAX_JOURNAL_SIZE_BYTES {
            return Err(SubjectGrantError::StoreUnavailable);
        }
        check_connection_storage_bounds(&connection, &database_path)?;
        if !schema_exists {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(map_store_error)?;
            check_write_headroom(&transaction, &database_path)?;
            create_schema(&transaction)?;
            validate_existing_schema(&transaction)?;
            check_connection_storage_bounds(&transaction, &database_path)?;
            transaction.commit().map_err(map_store_error)?;
        }
        let store = Self {
            connection,
            database_path,
        };
        store.check_storage_bounds()?;
        Ok(store)
    }

    /// Add a grant through an operator-controlled provisioning path.
    pub fn provision(&mut self, spec: &SubjectGrantSpec) -> Result<(), SubjectGrantError> {
        if !valid_spec(spec) {
            return Err(SubjectGrantError::Invalid);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_store_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        validate_existing_schema(&transaction)?;
        let count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM console_subject_grants", [], |row| {
                row.get(0)
            })
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        if count < 0 {
            return Err(SubjectGrantError::Corrupt);
        }
        if count >= MAX_SUBJECT_GRANTS as i64 {
            return Err(SubjectGrantError::Capacity);
        }
        transaction
            .execute(
                "INSERT INTO console_subject_grants (
                    grant_id, issuer, subject, permission,
                    project_id, run_id, episode_id, agent_id,
                    not_before, expires_at, revocation_generation, revoked
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, 0)",
                params![
                    spec.grant_id,
                    spec.issuer,
                    spec.subject,
                    spec.permission.as_str(),
                    spec.scope.project_id,
                    spec.scope.run_id,
                    spec.scope.episode_id,
                    spec.scope.agent_id,
                    to_sqlite_time(spec.not_before)?,
                    to_sqlite_time(spec.expires_at)?,
                ],
            )
            .map_err(map_store_error)?;
        check_connection_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_store_error)
    }

    /// Revoke a grant and increment its generation under the same SQLite writer lock used by
    /// [`SubjectGrantStore::reserve`]. Repeating a revocation leaves the generation unchanged.
    pub fn revoke(&mut self, grant_id: &str) -> Result<u64, SubjectGrantError> {
        if !crate::harness_facade::valid_id(grant_id) {
            return Err(SubjectGrantError::Invalid);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_store_error)?;
        check_write_headroom(&transaction, &self.database_path)?;
        validate_existing_schema(&transaction)?;
        let generation: Option<i64> = transaction
            .query_row(
                "SELECT revocation_generation FROM console_subject_grants WHERE grant_id = ?1",
                [grant_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        let Some(generation) = generation else {
            return Err(SubjectGrantError::NotFound);
        };
        let generation = u64::try_from(generation).map_err(|_| SubjectGrantError::Corrupt)?;
        let revoked: bool = transaction
            .query_row(
                "SELECT revoked FROM console_subject_grants WHERE grant_id = ?1",
                [grant_id],
                |row| row.get(0),
            )
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        let next_generation = if revoked {
            generation
        } else {
            generation
                .checked_add(1)
                .ok_or(SubjectGrantError::Corrupt)?
        };
        if !revoked {
            transaction
                .execute(
                    "UPDATE console_subject_grants
                     SET revoked = 1, revocation_generation = ?2 WHERE grant_id = ?1",
                    params![grant_id, to_sqlite_time(next_generation)?],
                )
                .map_err(map_store_error)?;
        }
        check_connection_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_store_error)?;
        Ok(next_generation)
    }

    pub fn checkpoint(&mut self) -> Result<(), SubjectGrantError> {
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        if busy != 0
            || log_frames < -1
            || checkpointed_frames < -1
            || (log_frames == -1) != (checkpointed_frames == -1)
            || (log_frames >= 0 && log_frames != checkpointed_frames)
        {
            return Err(SubjectGrantError::StoreUnavailable);
        }
        self.check_storage_bounds()
    }
}

impl SubjectGrantStore for SqliteSubjectGrantStore {
    fn reserve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
    ) -> Result<Vec<AdmittedSubjectGrant>, SubjectGrantError> {
        if !scope_is_valid(scope) || required.is_empty() || !unique_permissions(required, optional)
        {
            return Err(SubjectGrantError::Invalid);
        }
        self.check_storage_bounds()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_store_error)?;
        validate_existing_schema(&transaction)?;
        let mut admitted = Vec::with_capacity(required.len() + optional.len());
        for permission in required.iter().chain(optional.iter()) {
            let grant = find_admitted_grant(
                &transaction,
                principal.issuer(),
                principal.subject(),
                scope,
                *permission,
                now,
            )?;
            match (grant, required.contains(permission)) {
                (Some(grant), _) => admitted.push(grant),
                (None, true) => return Err(SubjectGrantError::Denied),
                (None, false) => {}
            }
        }
        check_connection_storage_bounds(&transaction, &self.database_path)?;
        transaction.commit().map_err(map_store_error)?;
        Ok(admitted)
    }
}

fn valid_spec(spec: &SubjectGrantSpec) -> bool {
    crate::harness_facade::valid_id(&spec.grant_id)
        && valid_bounded_text(&spec.issuer)
        && crate::harness_facade::valid_id(&spec.subject)
        && scope_is_valid(&spec.scope)
        && spec.not_before < spec.expires_at
        && i64::try_from(spec.expires_at).is_ok()
}

fn valid_bounded_text(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_GRANT_TEXT_BYTES
        && !value.contains('\0')
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn scope_is_valid(scope: &Scope) -> bool {
    [
        scope.project_id.as_str(),
        scope.run_id.as_str(),
        scope.episode_id.as_str(),
        scope.agent_id.as_str(),
    ]
    .iter()
    .all(|value| crate::harness_facade::valid_id(value))
}

impl SqliteSubjectGrantStore {
    fn check_storage_bounds(&self) -> Result<(), SubjectGrantError> {
        check_connection_storage_bounds(&self.connection, &self.database_path)
    }
}

fn unique_permissions(required: &[FacadePermission], optional: &[FacadePermission]) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    required
        .iter()
        .chain(optional.iter())
        .all(|permission| seen.insert(*permission))
}

fn to_sqlite_time(value: u64) -> Result<i64, SubjectGrantError> {
    i64::try_from(value).map_err(|_| SubjectGrantError::Invalid)
}

fn map_store_error(error: rusqlite::Error) -> SubjectGrantError {
    match error {
        rusqlite::Error::SqliteFailure(ref failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            SubjectGrantError::Duplicate
        }
        rusqlite::Error::SqliteFailure(ref failure, _)
            if failure.code == rusqlite::ErrorCode::DatabaseBusy
                || failure.code == rusqlite::ErrorCode::DatabaseLocked =>
        {
            SubjectGrantError::StoreUnavailable
        }
        _ => SubjectGrantError::StoreUnavailable,
    }
}
