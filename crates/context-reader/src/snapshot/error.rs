// SPDX-License-Identifier: MIT

use std::fmt;

use crate::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotError {
    Empty,
    TooLarge,
    Json,
    Invalid(&'static str),
}
impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("snapshot is empty"),
            Self::TooLarge => f.write_str("snapshot exceeds its byte bound"),
            Self::Json => f.write_str("invalid JSON"),
            Self::Invalid(field) => write!(f, "snapshot field is invalid: {field}"),
        }
    }
}
impl std::error::Error for SnapshotError {}
impl From<json::Error> for SnapshotError {
    fn from(_: json::Error) -> Self {
        Self::Json
    }
}
impl crate::json::AccessError for SnapshotError {
    fn invalid(field: &'static str) -> Self {
        Self::Invalid(field)
    }
}
