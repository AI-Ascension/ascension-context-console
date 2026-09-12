// SPDX-License-Identifier: MIT

use serde::Serialize;
use std::time::SystemTime;

use super::Store;
use super::error::ReadError;
use super::grant::{ReadGrant, valid_id};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompareResult {
    pub left_snapshot_id: String,
    pub right_snapshot_id: String,
    pub same_boundary: bool,
    pub same_component_order: bool,
    pub changed_components: Vec<String>,
}

impl Store {
    pub fn compare(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        left: &str,
        right: &str,
        now: SystemTime,
    ) -> Result<CompareResult, ReadError> {
        let left_bytes = self.get(token, grant, left, now)?;
        let right_bytes = self.get(token, grant, right, now)?;
        if left_bytes.len().saturating_add(right_bytes.len()) > self.config.max_compare_bytes {
            return Err(ReadError::TooLarge);
        }
        let left_snapshot = self.entries.get(left).ok_or(ReadError::NotFound)?;
        let right_snapshot = self.entries.get(right).ok_or(ReadError::NotFound)?;
        let left_components = left_snapshot.snapshot.components();
        let right_components = right_snapshot.snapshot.components();
        let mut changed_components = Vec::new();
        let max_components = left_components.len().max(right_components.len());
        for index in 0..max_components {
            let left_component = left_components.get(index);
            let right_component = right_components.get(index);
            let changed = match (left_component, right_component) {
                (Some(left), Some(right)) => {
                    left.component_id != right.component_id
                        || left.ordinal != right.ordinal
                        || left.observed_bytes != right.observed_bytes
                        || left.content_status != right.content_status
                        || left.sha256 != right.sha256
                }
                (Some(_), None) | (None, Some(_)) => true,
                (None, None) => false,
            };
            if changed && let Some(component) = left_component.or(right_component) {
                changed_components.push(component.component_id.clone());
            }
            if changed_components.len() >= 128 {
                break;
            }
        }
        Ok(CompareResult {
            left_snapshot_id: left.to_owned(),
            right_snapshot_id: right.to_owned(),
            same_boundary: left_snapshot.snapshot.projection().boundary
                == right_snapshot.snapshot.projection().boundary,
            same_component_order: left_components.len() == right_components.len()
                && left_components
                    .iter()
                    .zip(right_components.iter())
                    .all(|(left, right)| left.ordinal == right.ordinal),
            changed_components,
        })
    }

    pub fn compare_for_run(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        run_id: &str,
        left: &str,
        right: &str,
        now: SystemTime,
    ) -> Result<CompareResult, ReadError> {
        if !valid_id(run_id) {
            return Err(ReadError::InvalidScope);
        }
        self.authorize(token, grant, now)?;
        let left_entry = self.scoped_entry(grant, left)?;
        let right_entry = self.scoped_entry(grant, right)?;
        if left_entry.snapshot.projection().identity.run_id != run_id
            || right_entry.snapshot.projection().identity.run_id != run_id
        {
            return Err(ReadError::NotFound);
        }
        self.compare(token, grant, left, right, now)
    }
}
