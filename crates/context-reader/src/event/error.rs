// SPDX-License-Identifier: MIT

use std::fmt;

use crate::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventError {
    Empty,
    TooLarge,
    Json,
    Invalid(&'static str),
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("event is empty"),
            Self::TooLarge => f.write_str("event exceeds its byte bound"),
            Self::Json => f.write_str("invalid event JSON"),
            Self::Invalid(field) => write!(f, "event field is invalid: {field}"),
        }
    }
}

impl std::error::Error for EventError {}

impl From<json::Error> for EventError {
    fn from(_: json::Error) -> Self {
        Self::Json
    }
}

impl crate::json::AccessError for EventError {
    fn invalid(field: &'static str) -> Self {
        Self::Invalid(field)
    }
}
