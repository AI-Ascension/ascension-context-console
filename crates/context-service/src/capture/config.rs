// SPDX-License-Identifier: MIT

use crate::capture::CaptureError;

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
