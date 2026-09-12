// SPDX-License-Identifier: MIT

#[derive(Debug, Eq, PartialEq)]
pub enum IngestError {
    Empty,
    TooLarge,
    InvalidSnapshot(String),
    Capacity,
    Conflict,
    InvalidEvent(String),
    EventConflict,
    ContentConflict,
    PrivateContentUnsupported,
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("manifest is empty"),
            Self::TooLarge => formatter.write_str("manifest exceeds configured bound"),
            Self::InvalidSnapshot(message) => write!(formatter, "snapshot rejected: {message}"),
            Self::Capacity => formatter.write_str("snapshot capacity is full"),
            Self::Conflict => formatter.write_str("snapshot identity already has different bytes"),
            Self::InvalidEvent(message) => write!(formatter, "event rejected: {message}"),
            Self::EventConflict => {
                formatter.write_str("event identity already has different bytes")
            }
            Self::ContentConflict => {
                formatter.write_str("content reference already has different bytes")
            }
            Self::PrivateContentUnsupported => formatter
                .write_str("private snapshot content cannot be retained in the plaintext store"),
        }
    }
}

impl std::error::Error for IngestError {}

#[derive(Debug, Eq, PartialEq)]
pub enum ReadError {
    InvalidToken,
    InvalidScope,
    Expired,
    Forbidden,
    NotFound,
    TooLarge,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidToken => "invalid read token",
            Self::InvalidScope => "invalid read scope",
            Self::Expired => "read grant expired",
            Self::Forbidden => "read not permitted",
            Self::NotFound => "snapshot unavailable",
            Self::TooLarge => "comparison exceeds its bound",
        })
    }
}

impl std::error::Error for ReadError {}
