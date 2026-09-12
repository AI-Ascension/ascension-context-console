// SPDX-License-Identifier: MIT

use std::fmt::{Display, Formatter};

pub const CURRENT_DURABLE_STORE_SCHEMA_VERSION: i64 = 1;
pub(super) const STORE_SCHEMA: &str = "ascension.context-control.sqlite.v1";
pub(super) const AAD: &[u8] = b"ascension.context-control.sqlite.v1\0";
pub(super) const MAX_JOURNAL_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_EVENT_BYTES: usize = 64 * 1024;
pub(super) const MAX_EVENTS: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableStoreFailpoint {
    BeforeJournalWrite,
    BeforeOutbox,
    BeforeCommit,
    DiskFull,
    AfterCommitBeforePublication,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DurableStoreSnapshot {
    pub run_id: String,
    pub management_active: bool,
    pub journal_bytes: usize,
    pub journal_digest: String,
    pub phase1_snapshot_count: usize,
    pub phase1_snapshot_digests: Vec<String>,
    pub outbox_event_count: usize,
    pub outbox_event_digests: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DurableStoreError {
    InvalidPath,
    ParentMissing,
    InvalidKey,
    Sqlite,
    Encode,
    Decode,
    AuthenticationFailed,
    Corrupt,
    Incompatible,
    Missing,
    ScopeMismatch,
    TooLarge,
    Failpoint,
    SnapshotConflict,
    InvalidSnapshotId,
    InvalidMemoryBinding,
    MemoryBindingConflict,
    ManagementActive,
}

impl Display for DurableStoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath => "durable control store path is invalid",
            Self::ParentMissing => "durable control store parent is missing",
            Self::InvalidKey => "durable control store key is invalid",
            Self::Sqlite => "durable control store database operation failed",
            Self::Encode => "control journal could not be encoded",
            Self::Decode => "control journal could not be decoded",
            Self::AuthenticationFailed => "control journal authentication failed",
            Self::Corrupt => "durable control store integrity check failed",
            Self::Incompatible => "durable control store schema is incompatible",
            Self::Missing => "durable control journal is unavailable",
            Self::ScopeMismatch => "durable control journal scope does not match this store",
            Self::TooLarge => "durable control store object exceeds its bound",
            Self::Failpoint => "durable control store failpoint rejected the transaction",
            Self::SnapshotConflict => "Phase 1 snapshot identity already has different bytes",
            Self::InvalidSnapshotId => "Phase 1 snapshot identity is invalid",
            Self::InvalidMemoryBinding => "memory binding metadata is invalid",
            Self::MemoryBindingConflict => "memory binding identity already has different metadata",
            Self::ManagementActive => "legacy binary refused management-active state",
        })
    }
}

impl std::error::Error for DurableStoreError {}
