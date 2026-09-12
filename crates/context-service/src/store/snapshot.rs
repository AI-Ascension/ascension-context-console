// SPDX-License-Identifier: MIT

use context_reader::Snapshot;
use sha2::{Digest, Sha256};

use super::error::IngestError;
use super::summary::SnapshotSummary;
use super::{Store, StoredSnapshot};

impl Store {
    pub fn ingest(&mut self, bytes: &[u8]) -> Result<SnapshotSummary, IngestError> {
        if bytes.is_empty() {
            return Err(IngestError::Empty);
        }
        if bytes.len() > self.config.max_manifest_bytes {
            return Err(IngestError::TooLarge);
        }
        let snapshot = Snapshot::parse(bytes)
            .map_err(|error| IngestError::InvalidSnapshot(error.to_string()))?;
        if snapshot.projection().capture_mode == context_reader::CaptureMode::Private
            && snapshot
                .components()
                .iter()
                .any(|component| component.content_ref.is_some())
        {
            return Err(IngestError::PrivateContentUnsupported);
        }
        let id = snapshot.projection().snapshot_id.clone();
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if let Some(existing) = self.entries.get(&id) {
            if existing.digest != digest {
                return Err(IngestError::Conflict);
            }
            return Ok(SnapshotSummary::from(&existing.snapshot));
        }
        if self.entries.len() >= self.config.max_snapshots {
            return Err(IngestError::Capacity);
        }
        let summary = SnapshotSummary::from(&snapshot);
        self.entries.insert(
            id,
            StoredSnapshot {
                digest,
                bytes: bytes.to_vec(),
                snapshot,
            },
        );
        Ok(summary)
    }
}
