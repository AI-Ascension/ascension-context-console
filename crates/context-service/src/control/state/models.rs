// SPDX-License-Identifier: MIT

use super::super::render::PreparedMaterial;
use super::super::types::{Draft, Preview};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct DraftRecord {
    pub(super) draft: Draft,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PreviewRecord {
    pub(super) preview: Preview,
    pub(super) material: Option<PreparedMaterial>,
    pub(super) consumed: bool,
}
