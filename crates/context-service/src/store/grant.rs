// SPDX-License-Identifier: MIT

use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};

use context_reader::SnapshotProjection;

use super::Store;
use super::cursor::epoch_seconds;
use super::error::ReadError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePrivilege {
    Metadata,
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadGrant {
    pub(super) token_digest: [u8; 32],
    project: String,
    run: Option<String>,
    pub(super) privilege: CapturePrivilege,
    pub(super) expires_at: u64,
}

impl ReadGrant {
    pub fn issue(
        token: &[u8],
        project: impl Into<String>,
        run: Option<String>,
        privilege: CapturePrivilege,
        ttl: Duration,
        now: SystemTime,
    ) -> Result<Self, ReadError> {
        let project = project.into();
        if !valid_id(&project) {
            return Err(ReadError::InvalidScope);
        }
        if run.as_deref().is_some_and(|value| !valid_id(value)) {
            return Err(ReadError::InvalidScope);
        }
        if token.is_empty() || token.len() > 256 {
            return Err(ReadError::InvalidToken);
        }
        let expires_at = epoch_seconds(now)
            .checked_add(ttl.as_secs())
            .ok_or(ReadError::Expired)?;
        let token_digest: [u8; 32] = Sha256::digest(token).into();
        Ok(Self {
            token_digest,
            project,
            run,
            privilege,
            expires_at,
        })
    }

    pub(super) fn permits_scope(&self, snapshot: &SnapshotProjection) -> bool {
        self.project == snapshot.identity.agent_id
            && self
                .run
                .as_deref()
                .is_none_or(|run| run == snapshot.identity.run_id)
    }

    pub fn privilege(&self) -> CapturePrivilege {
        self.privilege
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn run(&self) -> Option<&str> {
        self.run.as_deref()
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    pub(super) fn valid_token(&self, token: &[u8], now: SystemTime) -> bool {
        if epoch_seconds(now) >= self.expires_at || token.is_empty() || token.len() > 256 {
            return false;
        }
        let digest: [u8; 32] = Sha256::digest(token).into();
        digest == self.token_digest
    }
}

impl Store {
    pub fn revoke(&mut self, token: &[u8]) -> Result<(), ReadError> {
        if token.is_empty() || token.len() > 256 {
            return Err(ReadError::InvalidToken);
        }
        let digest: [u8; 32] = Sha256::digest(token).into();
        self.revoked.insert(digest, ());
        Ok(())
    }
}

pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}
