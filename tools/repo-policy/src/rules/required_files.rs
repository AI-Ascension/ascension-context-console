// SPDX-License-Identifier: MIT

//! DOC001: required repository files.

use std::path::Path;

/// Required files, kept exactly as the historical list.
pub const REQUIRED: &[&str] = &[
    "AGENTS.md",
    "Cargo.toml",
    "Cargo.lock",
    "LICENSE",
    "README.md",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "clippy.toml",
    "policy.toml",
    "docs/ARCHITECTURE.md",
    "docs/CODING_STANDARDS.md",
    "docs/TESTING.md",
    "docs/COMPATIBILITY.md",
    "docs/LICENSING.md",
    "docs/WORKFLOWS.md",
    "docs/POLICY_AS_CODE.md",
    "docs/PRODUCT.md",
    "docs/SECURITY.md",
    "docs/REPOSITORY_LAYOUT.md",
    "contract-artifact/context-inspection-v1/context-snapshot.schema.json",
];

pub fn violations(root: &Path) -> Vec<String> {
    REQUIRED
        .iter()
        .filter(|relative| !root.join(relative).is_file())
        .map(|relative| format!("DOC001 missing required file: {relative}"))
        .collect()
}
