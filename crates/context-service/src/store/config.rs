// SPDX-License-Identifier: MIT

use super::{MAX_COMPARE_BYTES, MAX_MANIFEST_BYTES, MAX_SNAPSHOTS};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreConfig {
    pub max_snapshots: usize,
    pub max_manifest_bytes: usize,
    pub max_compare_bytes: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            max_snapshots: MAX_SNAPSHOTS,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
            max_compare_bytes: MAX_COMPARE_BYTES,
        }
    }
}
