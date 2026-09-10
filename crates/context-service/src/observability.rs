// SPDX-License-Identifier: MIT

//! Optional metadata-only telemetry.  The exporter never accepts content, hashes, bearer tokens,
//! raw errors or provider event payloads, and an exporter failure is intentionally fail-soft.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureTelemetry {
    pub boundary: String,
    pub mode: String,
    pub state: String,
    pub observed_bytes: u64,
    pub dropped_entries: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TelemetryError {
    Disabled,
    Rejected,
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disabled => "telemetry exporter is disabled",
            Self::Rejected => "telemetry field is not allowlisted",
        })
    }
}

impl std::error::Error for TelemetryError {}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporter_is_bounded_and_metadata_only() {
        let mut exporter = MemoryTelemetry::new(1).expect("bounded");
        exporter
            .export(&CaptureTelemetry {
                boundary: "adapter.cli_input".to_owned(),
                mode: "metadata".to_owned(),
                state: "prepared".to_owned(),
                observed_bytes: 12,
                dropped_entries: 0,
            })
            .expect("export");
        assert_eq!(exporter.records().len(), 1);
    }
}
