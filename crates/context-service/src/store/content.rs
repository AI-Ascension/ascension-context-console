// SPDX-License-Identifier: MIT

use sha2::{Digest, Sha256};
use std::time::SystemTime;

use super::cursor::hex_digest;
use super::error::{IngestError, ReadError};
use super::grant::{CapturePrivilege, ReadGrant, valid_id};
use super::{MAX_CONTENT_BYTES, MAX_CONTENT_REFS, MAX_MANIFEST_BYTES, Store};

impl Store {
    /// Retain one already-classified opaque component reference in bounded memory. The store
    /// never accepts filesystem paths or URLs and treats identical re-ingest as idempotent.
    pub fn ingest_content(&mut self, content_ref: &str, bytes: &[u8]) -> Result<(), IngestError> {
        if !valid_id(content_ref) || bytes.len() > MAX_MANIFEST_BYTES.saturating_mul(16) {
            return Err(IngestError::TooLarge);
        }
        if let Some(existing) = self.content.get(content_ref) {
            return if existing == bytes {
                Ok(())
            } else {
                Err(IngestError::ContentConflict)
            };
        }
        if self.content.len() >= MAX_CONTENT_REFS
            || self.content_bytes.saturating_add(bytes.len()) > MAX_CONTENT_BYTES
        {
            return Err(IngestError::Capacity);
        }
        self.content_bytes = self.content_bytes.saturating_add(bytes.len());
        self.content.insert(content_ref.to_owned(), bytes.to_vec());
        Ok(())
    }

    pub fn content(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        snapshot_id: &str,
        component_id: &str,
        now: SystemTime,
    ) -> Result<Vec<u8>, ReadError> {
        self.authorize(token, grant, now)?;
        let entry = self.scoped_entry(grant, snapshot_id)?;
        if grant.privilege != CapturePrivilege::Content {
            return Err(ReadError::Forbidden);
        }
        let component = entry
            .snapshot
            .components()
            .iter()
            .find(|component| component.component_id == component_id)
            .ok_or(ReadError::NotFound)?;
        if !self.content_available(grant, entry.snapshot.projection().capture_mode, component) {
            return Err(ReadError::NotFound);
        }
        let content_ref = component
            .content_ref
            .as_deref()
            .ok_or(ReadError::NotFound)?;
        self.content
            .get(content_ref)
            .cloned()
            .ok_or(ReadError::NotFound)
    }

    pub(super) fn content_available(
        &self,
        grant: &ReadGrant,
        capture_mode: context_reader::CaptureMode,
        component: &context_reader::Component,
    ) -> bool {
        let (Some(content_ref), Some(expected_digest)) = (
            component.content_ref.as_deref(),
            component.sha256.as_deref(),
        ) else {
            return false;
        };
        if grant.privilege != CapturePrivilege::Content
            || capture_mode == context_reader::CaptureMode::Private
            || matches!(
                component.content_status,
                context_reader::ComponentStatus::Expired
            )
        {
            return false;
        }
        self.content.get(content_ref).is_some_and(|bytes| {
            let digest: [u8; 32] = Sha256::digest(bytes).into();
            hex_digest(&digest) == expected_digest
        })
    }
}
