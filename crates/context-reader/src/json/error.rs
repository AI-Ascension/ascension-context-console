// SPDX-License-Identifier: MIT

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Unexpected,
    InvalidUtf8,
    InvalidEscape,
    InvalidNumber,
    DuplicateKey,
    TooDeep,
    TooManyItems,
    TrailingBytes,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Unexpected => "unexpected JSON input",
            Self::InvalidUtf8 => "JSON string is not UTF-8",
            Self::InvalidEscape => "invalid JSON string escape",
            Self::InvalidNumber => "JSON number is not a bounded unsigned integer",
            Self::DuplicateKey => "JSON object contains a duplicate key",
            Self::TooDeep => "JSON nesting exceeds its bound",
            Self::TooManyItems => "JSON object or array exceeds its item bound",
            Self::TrailingBytes => "JSON has trailing non-whitespace bytes",
        };
        formatter.write_str(message)
    }
}
