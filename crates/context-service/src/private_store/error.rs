// SPDX-License-Identifier: MIT

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrivateStoreError {
    PolicyNotApproved,
    InvalidKey,
    InvalidReference,
    TooLarge,
    Quota,
    NotFound,
    Unauthorized,
    AuthenticationFailed,
}

impl std::fmt::Display for PrivateStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::PolicyNotApproved => "private capture policy is not approved",
            Self::InvalidKey => "private capture key is invalid",
            Self::InvalidReference => "content reference is invalid",
            Self::TooLarge => "private content exceeds its bound",
            Self::Quota => "private content quota is full",
            Self::NotFound => "private content is unavailable",
            Self::Unauthorized => "private content authorization is missing",
            Self::AuthenticationFailed => "private content authentication failed",
        })
    }
}

impl std::error::Error for PrivateStoreError {}
