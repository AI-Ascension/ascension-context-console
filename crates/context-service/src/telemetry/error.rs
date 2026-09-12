// SPDX-License-Identifier: MIT

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
