// SPDX-License-Identifier: MIT

//! Optional metadata-only telemetry.  The exporter never accepts content, hashes, bearer tokens,
//! raw errors or provider event payloads, and an exporter failure is intentionally fail-soft.

mod capture;
mod error;
mod memory;
#[cfg(test)]
mod tests;

pub use capture::{CaptureTelemetry, NoopTelemetry, TelemetryExporter};
pub use error::TelemetryError;
pub use memory::MemoryTelemetry;
