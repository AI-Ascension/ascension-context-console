// SPDX-License-Identifier: MIT

mod details;
mod error;
mod parse;
#[cfg(test)]
mod tests;
mod types;

pub use error::EventError;
pub use parse::{parse_event, parse_event_lines};
pub use types::{CaptureEvent, EventDetails, EventType};
