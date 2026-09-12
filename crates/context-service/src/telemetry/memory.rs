// SPDX-License-Identifier: MIT

use crate::telemetry::{CaptureTelemetry, TelemetryError, TelemetryExporter};

#[derive(Default)]
pub struct MemoryTelemetry {
    records: Vec<CaptureTelemetry>,
    max_records: usize,
}

impl MemoryTelemetry {
    pub fn new(max_records: usize) -> Result<Self, TelemetryError> {
        if max_records == 0 || max_records > 128 {
            return Err(TelemetryError::Rejected);
        }
        Ok(Self {
            records: Vec::new(),
            max_records,
        })
    }

    pub fn records(&self) -> &[CaptureTelemetry] {
        &self.records
    }
}

impl TelemetryExporter for MemoryTelemetry {
    fn export(&mut self, telemetry: &CaptureTelemetry) -> Result<(), TelemetryError> {
        if telemetry.boundary.len() > 64
            || telemetry.mode.len() > 16
            || telemetry.state.len() > 32
            || telemetry.boundary.chars().any(char::is_control)
            || telemetry.mode.chars().any(char::is_control)
            || telemetry.state.chars().any(char::is_control)
        {
            return Err(TelemetryError::Rejected);
        }
        if self.records.len() >= self.max_records {
            self.records.remove(0);
        }
        self.records.push(telemetry.clone());
        Ok(())
    }
}
