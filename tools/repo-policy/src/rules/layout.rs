// SPDX-License-Identifier: MIT

//! LAYOUT001: required top-level directories.

use std::path::Path;

/// Locations that must exist for the workspace layout to hold.
pub const DIRECTORIES: &[&str] = &["crates", "tools"];

pub fn violations(root: &Path) -> Vec<String> {
    DIRECTORIES
        .iter()
        .filter(|relative| !root.join(relative).is_dir())
        .map(|relative| format!("LAYOUT001 missing directory: {relative}"))
        .collect()
}
