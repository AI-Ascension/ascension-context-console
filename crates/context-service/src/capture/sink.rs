// SPDX-License-Identifier: MIT

use crate::capture::{CaptureError, PreparedCapture};

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
