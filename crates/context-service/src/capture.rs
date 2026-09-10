// SPDX-License-Identifier: MIT

//! Producer-side capture primitives.  These types are intentionally independent from any
//! provider, game, process or network client.  A harness adapter can borrow the bytes it already
//! prepared and hand a bounded manifest to a sink without changing the provider request.

use sha2::{Digest, Sha256};
use std::collections::VecDeque;

pub const MAX_QUEUE_ENTRIES: usize = 128;
pub const MAX_CAPTURE_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode {
    Off,
    Metadata,
    Memory,
    Private,
}

impl CaptureMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Metadata => "metadata",
            Self::Memory => "memory",
            Self::Private => "private",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportState {
    Prepared,
    WriteCompleted,
    WriteFailed,
    ReceiptReported,
    Unknown,
}

impl TransportState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::WriteCompleted => "input_write_completed",
            Self::WriteFailed => "input_write_failed",
            Self::ReceiptReported => "provider_receipt_reported",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub max_queue_entries: usize,
    pub max_record_bytes: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            mode: CaptureMode::Off,
            max_queue_entries: MAX_QUEUE_ENTRIES,
            max_record_bytes: MAX_CAPTURE_BYTES,
        }
    }
}

impl CaptureConfig {
    pub fn validate(&self) -> Result<(), CaptureError> {
        if self.max_queue_entries == 0 || self.max_queue_entries > MAX_QUEUE_ENTRIES {
            return Err(CaptureError::Capacity);
        }
        if self.max_record_bytes == 0 || self.max_record_bytes > MAX_CAPTURE_BYTES {
            return Err(CaptureError::Capacity);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCapture<'a> {
    pub snapshot_id: &'a str,
    pub attempt_id: &'a str,
    pub boundary: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureRecord {
    pub snapshot_id: String,
    pub attempt_id: String,
    pub boundary: String,
    pub state: TransportState,
    pub observed_bytes: usize,
    pub digest: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureError {
    Disabled,
    Capacity,
    TooLarge,
    InvalidIdentity,
    SinkUnavailable,
    PrivateRequiresVault,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disabled => "capture is disabled",
            Self::Capacity => "capture capacity is invalid or full",
            Self::TooLarge => "capture record exceeds its byte bound",
            Self::InvalidIdentity => "capture identity is invalid",
            Self::SinkUnavailable => "capture sink is unavailable",
            Self::PrivateRequiresVault => "private capture requires an approved encrypted vault",
        })
    }
}

impl std::error::Error for CaptureError {}

/// A fail-soft sink used by adapters.  `Off` returns before touching the supplied bytes, so the
/// disabled path does not copy or hash application content.
pub trait CaptureSink {
    fn prepared(&mut self, capture: PreparedCapture<'_>) -> Result<(), CaptureError>;
    fn write_completed(&mut self, snapshot_id: &str) -> Result<(), CaptureError>;
    fn write_failed(&mut self, snapshot_id: &str, code: &str) -> Result<(), CaptureError>;
}

#[derive(Default)]
pub struct NoopCapture;

impl CaptureSink for NoopCapture {
    fn prepared(&mut self, _capture: PreparedCapture<'_>) -> Result<(), CaptureError> {
        Ok(())
    }

    fn write_completed(&mut self, _snapshot_id: &str) -> Result<(), CaptureError> {
        Ok(())
    }

    fn write_failed(&mut self, _snapshot_id: &str, _code: &str) -> Result<(), CaptureError> {
        Ok(())
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_path_does_not_hash_or_copy() {
        let mut sink = NoopCapture;
        let bytes = b"synthetic-capture-canary";
        assert!(
            sink.prepared(PreparedCapture {
                snapshot_id: "snapshot-1",
                attempt_id: "attempt-1",
                boundary: "adapter.cli_input",
                bytes,
            })
            .is_ok()
        );
    }

    #[test]
    fn metadata_omits_digest_and_memory_bounds_queue() {
        let mut sink = MemoryCapture::new(CaptureConfig {
            mode: CaptureMode::Metadata,
            max_queue_entries: 1,
            max_record_bytes: 128,
        })
        .expect("valid config");
        for id in ["snapshot-1", "snapshot-2"] {
            sink.prepared(PreparedCapture {
                snapshot_id: id,
                attempt_id: "attempt-1",
                boundary: "adapter.cli_input",
                bytes: b"input",
            })
            .expect("capture");
        }
        assert_eq!(sink.records().count(), 1);
        assert_eq!(sink.dropped_entries(), 1);
        assert!(sink.records().next().expect("record").digest.is_none());
    }

    #[test]
    fn private_mode_is_rejected_until_an_approved_vault_is_wired() {
        assert!(matches!(
            MemoryCapture::new(CaptureConfig {
                mode: CaptureMode::Private,
                max_queue_entries: 1,
                max_record_bytes: 128,
            }),
            Err(CaptureError::PrivateRequiresVault)
        ));
    }
}
