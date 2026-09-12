// SPDX-License-Identifier: MIT

use std::time::SystemTime;

use super::cursor::epoch_seconds;
use super::error::ReadError;
use super::grant::{CapturePrivilege, ReadGrant};
use super::summary::{ComponentSummary, SnapshotSummary};
use super::{Store, StoredSnapshot};

impl Store {
    pub fn component(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        snapshot_id: &str,
        component_id: &str,
        now: SystemTime,
    ) -> Result<ComponentSummary, ReadError> {
        self.authorize(token, grant, now)?;
        let entry = self.scoped_entry(grant, snapshot_id)?;
        let component = entry
            .snapshot
            .components()
            .iter()
            .find(|component| component.component_id == component_id)
            .ok_or(ReadError::NotFound)?;
        let content_available =
            self.content_available(grant, entry.snapshot.projection().capture_mode, component);
        Ok(ComponentSummary {
            snapshot_id: snapshot_id.to_owned(),
            component_id: component.component_id.clone(),
            ordinal: component.ordinal,
            kind: component.kind.clone(),
            role: component.role.clone(),
            media_type: component.media_type.clone(),
            observed_bytes: component.observed_bytes,
            content_status: component.content_status.as_str().to_owned(),
            content_available,
            digest_present: component.sha256.is_some(),
            measurement: component.measurement.clone(),
        })
    }

    pub fn list(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        now: SystemTime,
        limit: usize,
    ) -> Result<Vec<SnapshotSummary>, ReadError> {
        if limit == 0 || limit > 200 {
            return Err(ReadError::TooLarge);
        }
        self.authorize(token, grant, now)?;
        let mut result = Vec::new();
        for entry in self.entries.values() {
            if grant.permits_scope(entry.snapshot.projection()) {
                result.push(SnapshotSummary::from(&entry.snapshot));
                if result.len() == limit {
                    break;
                }
            }
        }
        Ok(result)
    }

    pub fn get(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        snapshot_id: &str,
        now: SystemTime,
    ) -> Result<Vec<u8>, ReadError> {
        self.authorize(token, grant, now)?;
        let entry = self.scoped_entry(grant, snapshot_id)?;
        if grant.privilege == CapturePrivilege::Metadata
            && entry
                .snapshot
                .components()
                .iter()
                .any(|component| component.content_ref.is_some())
        {
            return Err(ReadError::Forbidden);
        }
        Ok(entry.bytes.clone())
    }

    pub fn authorize(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        now: SystemTime,
    ) -> Result<(), ReadError> {
        if !grant.valid_token(token, now) || self.revoked.contains_key(&grant.token_digest) {
            return Err(if epoch_seconds(now) >= grant.expires_at {
                ReadError::Expired
            } else {
                ReadError::Forbidden
            });
        }
        Ok(())
    }

    pub(super) fn scoped_entry(
        &self,
        grant: &ReadGrant,
        snapshot_id: &str,
    ) -> Result<&StoredSnapshot, ReadError> {
        let entry = self.entries.get(snapshot_id).ok_or(ReadError::NotFound)?;
        if grant.permits_scope(entry.snapshot.projection()) {
            Ok(entry)
        } else {
            Err(ReadError::NotFound)
        }
    }
}
