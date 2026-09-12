// SPDX-License-Identifier: MIT

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureError {
    Disabled,
    Capacity,
    TooLarge,
    InvalidIdentity,
    SinkUnavailable,
    PrivateRequiresVault,
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disabled => "capture is disabled",
            Self::Capacity => "capture capacity is invalid or full",
            Self::TooLarge => "capture record exceeds its byte bound",
            Self::InvalidIdentity => "capture identity is invalid",
            Self::SinkUnavailable => "capture sink is unavailable",
            Self::PrivateRequiresVault => "private capture requires an approved encrypted vault",
        })
    }
}

impl std::error::Error for CaptureError {}
