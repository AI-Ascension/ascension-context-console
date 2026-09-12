// SPDX-License-Identifier: MIT

//! Producer-side capture primitives.  These types are intentionally independent from any
//! provider, game, process or network client.  A harness adapter can borrow the bytes it already
//! prepared and hand a bounded manifest to a sink without changing the provider request.

mod config;
mod error;
mod memory;
mod record;
mod sink;
#[cfg(test)]
mod tests;

pub use config::{CaptureConfig, CaptureMode};
pub use error::CaptureError;
pub use memory::MemoryCapture;
pub use record::{CaptureRecord, PreparedCapture, TransportState};
pub use sink::{CaptureSink, NoopCapture};
