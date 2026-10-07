use serde::Serialize;
use sha2::{Digest, Sha256};

use super::validation::{OwnerWireError, bounded_json_bytes, validate_digest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DigestHex(String);

impl DigestHex {
    pub fn parse(value: impl Into<String>) -> Result<Self, HarnessDigestError> {
        let value = value.into();
        validate_digest("sha256", &value).map_err(|_| HarnessDigestError::InvalidDigest)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessDigestError {
    InvalidDigest,
    InvalidRequest,
}

impl std::fmt::Display for HarnessDigestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDigest => f.write_str("digest must be lowercase SHA-256 hex"),
            Self::InvalidRequest => f.write_str("request cannot be encoded for its digest"),
        }
    }
}

impl std::error::Error for HarnessDigestError {}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

pub(crate) fn serialized_request_digest<T: Serialize>(
    request: &T,
) -> Result<String, HarnessDigestError> {
    let bytes = bounded_json_bytes(request).map_err(map_wire_error)?;
    Ok(sha256_hex(&bytes))
}

fn map_wire_error(_error: OwnerWireError) -> HarnessDigestError {
    HarnessDigestError::InvalidRequest
}
