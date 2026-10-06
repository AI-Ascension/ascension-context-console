// SPDX-License-Identifier: MIT

use super::super::{
    MAX_SUBJECT_GRANTS, SqliteSubjectGrantStore, SubjectGrantError, SubjectGrantSpec,
};
use super::{
    MAX_DATABASE_BYTES, MAX_SINGLE_WRITE_GROWTH_BYTES, MAX_WAL_BYTES, REQUIRED_PAGE_SIZE,
    bounded_file_size, find_admitted_grant,
};
use crate::control::Scope;
use crate::harness_facade::FacadePermission;
use rusqlite::{Connection, TransactionBehavior};
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DATABASE: AtomicU64 = AtomicU64::new(0);

struct TestDatabase {
    directory: PathBuf,
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        let unique = NEXT_DATABASE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "console-subject-grants-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).expect("create private test directory");
        let path = directory.join("grants.sqlite");
        Self { directory, path }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn scope() -> Scope {
    Scope {
        project_id: "project-a".to_owned(),
        run_id: "run-a".to_owned(),
        episode_id: "episode-a".to_owned(),
        agent_id: "agent-a".to_owned(),
    }
}

fn provision(store: &mut SqliteSubjectGrantStore) {
    store
        .provision(&SubjectGrantSpec {
            grant_id: "grant-a".to_owned(),
            issuer: "https://issuer.example".to_owned(),
            subject: "console-user".to_owned(),
            permission: FacadePermission::MetadataRead,
            scope: scope(),
            not_before: 10,
            expires_at: 100,
        })
        .expect("provision fixture grant");
}

fn corrupt_row(column: &str, value: i64) {
    let database = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&database.path).expect("open test store");
    provision(&mut store);
    store
        .connection
        .execute(
            &format!("UPDATE console_subject_grants SET {column} = ?1"),
            [value],
        )
        .expect("inject malformed stored integer");
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .expect("begin lookup transaction");
    let result = find_admitted_grant(
        &transaction,
        "https://issuer.example",
        "console-user",
        &scope(),
        FacadePermission::MetadataRead,
        20,
    );
    assert_eq!(result, Err(SubjectGrantError::Corrupt), "column {column}");
}

#[test]
fn malformed_negative_grant_times_and_generation_fail_closed() {
    corrupt_row("not_before", -1);
    corrupt_row("expires_at", -1);
    corrupt_row("revocation_generation", -1);
}

#[test]
fn database_and_wal_byte_limits_are_checked() {
    let directory = TestDatabase::new();
    let oversized = directory.directory.join("oversized.sqlite-wal");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&oversized)
        .expect("create sparse WAL fixture")
        .set_len(MAX_WAL_BYTES + 1)
        .expect("size sparse WAL fixture");
    assert_eq!(
        bounded_file_size(&oversized, MAX_WAL_BYTES),
        Err(SubjectGrantError::StorageLimit)
    );

    let oversized_database = directory.directory.join("oversized.sqlite");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&oversized_database)
        .expect("create sparse database fixture")
        .set_len(MAX_DATABASE_BYTES + 1)
        .expect("size sparse database fixture");
    assert_eq!(
        bounded_file_size(&oversized_database, MAX_DATABASE_BYTES),
        Err(SubjectGrantError::StorageLimit)
    );
}

#[test]
fn checkpoint_reports_success_only_after_sqlite_checkpoint_and_size_checks() {
    let database = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&database.path).expect("open test store");
    provision(&mut store);
    store.checkpoint().expect("checkpoint WAL");
    store
        .check_storage_bounds()
        .expect("bounded post-checkpoint files");
}

#[test]
fn nondefault_database_page_size_is_rejected_before_schema_mutation() {
    let database = TestDatabase::new();
    let connection = Connection::open(&database.path).expect("create nondefault database");
    connection
        .pragma_update(None, "page_size", 8192_u32)
        .expect("select nondefault page size");
    connection
        .execute_batch("VACUUM;")
        .expect("persist page size");
    drop(connection);
    let before = fs::read(&database.path).expect("read database before refusal");

    assert_eq!(
        SqliteSubjectGrantStore::open(&database.path).err(),
        Some(SubjectGrantError::StoreUnavailable)
    );
    assert_eq!(
        fs::read(&database.path).expect("read database after refusal"),
        before,
        "unsupported page size must be rejected before schema writes"
    );
}

#[test]
fn long_lived_reader_keeps_wal_bounded_and_refuses_without_write_headroom() {
    let database = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&database.path).expect("open grant store");
    let configured_page_size: u32 = store
        .connection
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .expect("read page size");
    assert_eq!(configured_page_size, REQUIRED_PAGE_SIZE);
    store
        .connection
        .pragma_update(None, "wal_autocheckpoint", 0_u32)
        .expect("disable automatic checkpoint for held-reader fixture");

    let reader = Connection::open(&database.path).expect("open long-lived reader");
    reader
        .execute_batch("BEGIN DEFERRED;")
        .expect("begin reader");
    let _: i64 = reader
        .query_row("SELECT COUNT(*) FROM console_subject_grants", [], |row| {
            row.get(0)
        })
        .expect("pin reader snapshot before writes");

    let mut successful_writes = 0usize;
    let mut refused = false;
    for index in 0..MAX_SUBJECT_GRANTS {
        let result = store.provision(&SubjectGrantSpec {
            grant_id: format!("grant-{index:04}"),
            issuer: "https://issuer.example".to_owned(),
            subject: "console-user".to_owned(),
            permission: FacadePermission::MetadataRead,
            scope: scope(),
            not_before: 10,
            expires_at: 100,
        });
        match result {
            Ok(()) => successful_writes += 1,
            Err(SubjectGrantError::StorageLimit) => {
                refused = true;
                break;
            }
            Err(error) => panic!("unexpected provisioning result: {error:?}"),
        }
    }
    assert!(
        refused,
        "a held reader must eventually consume bounded WAL headroom"
    );
    assert!(successful_writes > 0);

    let wal_path = {
        let mut value = database.path.as_os_str().to_os_string();
        value.push("-wal");
        PathBuf::from(value)
    };
    let wal_bytes = fs::metadata(&wal_path)
        .expect("held reader keeps WAL file present")
        .len();
    assert!(
        wal_bytes > MAX_WAL_BYTES - MAX_SINGLE_WRITE_GROWTH_BYTES,
        "fixture reaches the guarded headroom threshold"
    );
    assert!(
        wal_bytes <= MAX_WAL_BYTES,
        "WAL stays under its hard byte cap"
    );
    let stored_count: i64 = store
        .connection
        .query_row("SELECT COUNT(*) FROM console_subject_grants", [], |row| {
            row.get(0)
        })
        .expect("count committed grants after refusal");
    assert_eq!(stored_count as usize, successful_writes);

    drop(reader);
    store.checkpoint().expect("checkpoint after reader closes");
}

#[test]
fn existing_store_rejects_extra_schema_objects_and_altered_definitions() {
    let with_trigger = TestDatabase::new();
    let store = SqliteSubjectGrantStore::open(&with_trigger.path).expect("create store schema");
    drop(store);
    let connection = Connection::open(&with_trigger.path).expect("open raw schema connection");
    connection
        .execute_batch(
            "CREATE TRIGGER extra_grant_trigger BEFORE INSERT ON console_subject_grants
             BEGIN SELECT 1; END;",
        )
        .expect("add unexpected trigger");
    drop(connection);
    assert_eq!(
        SqliteSubjectGrantStore::open(&with_trigger.path).err(),
        Some(SubjectGrantError::Corrupt)
    );

    let altered = TestDatabase::new();
    let connection = Connection::open(&altered.path).expect("create altered schema");
    connection
        .execute_batch(
            "CREATE TABLE console_subject_grants (
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
                 revoked INTEGER NOT NULL
             );
             CREATE INDEX console_subject_grants_lookup
             ON console_subject_grants (
                 issuer, subject, project_id, run_id, episode_id, agent_id, permission
             );",
        )
        .expect("create altered table without required CHECK constraint");
    drop(connection);
    assert_eq!(
        SqliteSubjectGrantStore::open(&altered.path).err(),
        Some(SubjectGrantError::Corrupt)
    );
}

#[test]
fn existing_rows_reject_oversized_text_wrong_storage_types_and_invalid_scope_ids() {
    let oversized = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&oversized.path).expect("create grant store");
    provision(&mut store);
    store
        .connection
        .execute(
            "UPDATE console_subject_grants SET issuer = ?1",
            ["i".repeat(129)],
        )
        .expect("inject oversized indexed text");
    drop(store);
    assert_eq!(
        SqliteSubjectGrantStore::open(&oversized.path).err(),
        Some(SubjectGrantError::Corrupt)
    );

    let wrong_type = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&wrong_type.path).expect("create grant store");
    provision(&mut store);
    store
        .connection
        .execute(
            "UPDATE console_subject_grants SET issuer = X'697373756572'",
            [],
        )
        .expect("inject non-TEXT indexed identity");
    drop(store);
    assert_eq!(
        SqliteSubjectGrantStore::open(&wrong_type.path).err(),
        Some(SubjectGrantError::Corrupt)
    );

    let invalid_scope = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&invalid_scope.path).expect("create grant store");
    provision(&mut store);
    store
        .connection
        .execute(
            "UPDATE console_subject_grants SET project_id = 'not a scope id'",
            [],
        )
        .expect("inject invalid bounded scope text");
    drop(store);
    assert_eq!(
        SqliteSubjectGrantStore::open(&invalid_scope.path).err(),
        Some(SubjectGrantError::Corrupt)
    );
}

#[test]
fn existing_grant_count_above_capacity_is_typed_corruption() {
    let database = TestDatabase::new();
    let mut store = SqliteSubjectGrantStore::open(&database.path).expect("open store");
    let transaction = store
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("begin fixture insert");
    for index in 0..=MAX_SUBJECT_GRANTS {
        transaction
            .execute(
                "INSERT INTO console_subject_grants (
                    grant_id, issuer, subject, permission,
                    project_id, run_id, episode_id, agent_id,
                    not_before, expires_at, revocation_generation, revoked
                 ) VALUES (?1, 'https://issuer.example', 'console-user', 'context.metadata.read',
                           'project-a', 'run-a', 'episode-a', 'agent-a', 10, 100, 0, 0)",
                [format!("grant-{index:04}")],
            )
            .expect("insert preexisting over-capacity grant");
    }
    transaction.commit().expect("commit over-capacity fixture");
    store
        .checkpoint()
        .expect("truncate over-capacity fixture WAL");
    drop(store);

    assert_eq!(
        SqliteSubjectGrantStore::open(&database.path).err(),
        Some(SubjectGrantError::Corrupt)
    );
}
