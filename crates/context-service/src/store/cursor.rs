// SPDX-License-Identifier: MIT

use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

use super::error::ReadError;
use super::grant::ReadGrant;

pub(super) fn epoch_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(super) fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut value = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

pub(super) fn encode_event_cursor(grant: &ReadGrant, run_id: &str, sequence: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"ascension.context-event-cursor.v1\0");
    hasher.update(grant.token_digest);
    for value in [grant.project(), run_id] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.update(sequence.to_be_bytes());
    let digest: [u8; 32] = hasher.finalize().into();
    format!("c1-{sequence}-{}", hex_digest(&digest))
}

pub(super) fn decode_event_cursor(
    grant: &ReadGrant,
    run_id: &str,
    cursor: &str,
) -> Result<u64, ReadError> {
    let Some(value) = cursor.strip_prefix("c1-") else {
        return Err(ReadError::InvalidScope);
    };
    let Some((sequence, _digest)) = value.split_once('-') else {
        return Err(ReadError::InvalidScope);
    };
    let sequence = sequence
        .parse::<u64>()
        .map_err(|_| ReadError::InvalidScope)?;
    if encode_event_cursor(grant, run_id, sequence) != cursor {
        return Err(ReadError::InvalidScope);
    }
    Ok(sequence)
}
