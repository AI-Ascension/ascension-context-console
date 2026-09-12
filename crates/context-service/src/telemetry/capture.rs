// SPDX-License-Identifier: MIT

use crate::telemetry::TelemetryError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureTelemetry {
    pub boundary: String,
    pub mode: String,
    pub state: String,
    pub observed_bytes: u64,
    pub dropped_entries: u64,
}

pub trait TelemetryExporter {
    fn export(&mut self, telemetry: &CaptureTelemetry) -> Result<(), TelemetryError>;
}

#[derive(Default)]
pub struct NoopTelemetry;

impl TelemetryExporter for NoopTelemetry {
    fn export(&mut self, _telemetry: &CaptureTelemetry) -> Result<(), TelemetryError> {
        Ok(())
    }
}
