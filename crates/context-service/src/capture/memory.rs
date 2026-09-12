// SPDX-License-Identifier: MIT

use crate::capture::{
    CaptureConfig, CaptureError, CaptureMode, CaptureRecord, CaptureSink, PreparedCapture,
    TransportState,
};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

/// Bounded memory-only capture used for local inspection and tests.  It retains metadata and a
/// digest, never writes a fallback file, and reports queue overflow as a gap.
pub struct MemoryCapture {
    config: CaptureConfig,
    records: VecDeque<CaptureRecord>,
    dropped: u64,
}

impl MemoryCapture {
    pub fn new(config: CaptureConfig) -> Result<Self, CaptureError> {
        config.validate()?;
        if config.mode == CaptureMode::Off {
            return Err(CaptureError::Disabled);
        }
        if config.mode == CaptureMode::Private {
            return Err(CaptureError::PrivateRequiresVault);
        }
        Ok(Self {
            config,
            records: VecDeque::new(),
            dropped: 0,
        })
    }

    pub fn records(&self) -> impl Iterator<Item = &CaptureRecord> {
        self.records.iter()
    }

    pub const fn dropped_entries(&self) -> u64 {
        self.dropped
    }

    fn push(&mut self, record: CaptureRecord) {
        if self.records.len() >= self.config.max_queue_entries {
            self.records.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.records.push_back(record);
    }

    fn id(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 128
            && value.chars().enumerate().all(|(index, character)| {
                character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
            })
    }
}

impl CaptureSink for MemoryCapture {
    fn prepared(&mut self, capture: PreparedCapture<'_>) -> Result<(), CaptureError> {
        if !Self::id(capture.snapshot_id)
            || !Self::id(capture.attempt_id)
            || capture.boundary.is_empty()
        {
            return Err(CaptureError::InvalidIdentity);
        }
        if capture.bytes.len() > self.config.max_record_bytes {
            return Err(CaptureError::TooLarge);
        }
        let digest = if self.config.mode == CaptureMode::Metadata {
            None
        } else {
            Some(Sha256::digest(capture.bytes).into())
        };
        self.push(CaptureRecord {
            snapshot_id: capture.snapshot_id.to_owned(),
            attempt_id: capture.attempt_id.to_owned(),
            boundary: capture.boundary.to_owned(),
            state: TransportState::Prepared,
            observed_bytes: capture.bytes.len(),
            digest,
        });
        Ok(())
    }

    fn write_completed(&mut self, snapshot_id: &str) -> Result<(), CaptureError> {
        if !Self::id(snapshot_id) {
            return Err(CaptureError::InvalidIdentity);
        }
        self.push(CaptureRecord {
            snapshot_id: snapshot_id.to_owned(),
            attempt_id: String::new(),
            boundary: String::new(),
            state: TransportState::WriteCompleted,
            observed_bytes: 0,
            digest: None,
        });
        Ok(())
    }

    fn write_failed(&mut self, snapshot_id: &str, _code: &str) -> Result<(), CaptureError> {
        if !Self::id(snapshot_id) {
            return Err(CaptureError::InvalidIdentity);
        }
        self.push(CaptureRecord {
            snapshot_id: snapshot_id.to_owned(),
            attempt_id: String::new(),
            boundary: String::new(),
            state: TransportState::WriteFailed,
            observed_bytes: 0,
            digest: None,
        });
        Ok(())
    }
}
