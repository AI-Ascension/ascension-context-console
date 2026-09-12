// SPDX-License-Identifier: MIT

use context_reader::{CaptureEvent, Snapshot};
use std::collections::BTreeMap;

mod compare;
mod config;
mod content;
mod cursor;
mod error;
mod event;
mod grant;
mod query;
mod snapshot;
mod summary;
#[cfg(test)]
mod tests;

pub use self::compare::CompareResult;
pub use self::config::StoreConfig;
pub use self::error::{IngestError, ReadError};
pub use self::grant::{CapturePrivilege, ReadGrant};
pub use self::summary::{ComponentSummary, EventPage, SnapshotSummary};

pub const MAX_SNAPSHOTS: usize = 128;
pub const MAX_MANIFEST_BYTES: usize = 1_048_576;
pub const MAX_COMPARE_BYTES: usize = 64 * 1024;
pub const MAX_EVENTS: usize = 4096;
pub const MAX_CONTENT_REFS: usize = 512;
pub const MAX_CONTENT_BYTES: usize = 512 * 1024 * 1024;

#[derive(Default)]
pub struct Store {
    config: StoreConfig,
    entries: BTreeMap<String, StoredSnapshot>,
    events: BTreeMap<String, StoredEvent>,
    event_scope_sequences: BTreeMap<(String, String), u64>,
    content: BTreeMap<String, Vec<u8>>,
    content_bytes: usize,
    revoked: BTreeMap<[u8; 32], ()>,
}

struct StoredSnapshot {
    digest: [u8; 32],
    bytes: Vec<u8>,
    snapshot: Snapshot,
}

struct StoredEvent {
    digest: [u8; 32],
    event: CaptureEvent,
    scope: Option<(String, String)>,
    scope_sequence: Option<u64>,
}

impl Store {
    pub fn with_config(config: StoreConfig) -> Result<Self, IngestError> {
        if config.max_snapshots == 0
            || config.max_snapshots > MAX_SNAPSHOTS
            || config.max_manifest_bytes == 0
            || config.max_manifest_bytes > MAX_MANIFEST_BYTES
            || config.max_compare_bytes == 0
            || config.max_compare_bytes > MAX_COMPARE_BYTES
        {
            return Err(IngestError::Capacity);
        }
        Ok(Self {
            config,
            entries: BTreeMap::new(),
            events: BTreeMap::new(),
            event_scope_sequences: BTreeMap::new(),
            content: BTreeMap::new(),
            content_bytes: 0,
            revoked: BTreeMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    pub fn content_len(&self) -> usize {
        self.content.len()
    }
}
