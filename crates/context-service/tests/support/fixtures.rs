// SPDX-License-Identifier: MIT

//! Fixture-path helpers that resolve repository fixtures from any crate-relative test target.

use std::path::{Path, PathBuf};

/// Repository root, derived from the crate manifest directory (`crates/context-service`).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .to_path_buf()
}

/// Absolute path to a repository-relative fixture, such as `fixtures/valid/snapshot-cli.json`.
pub fn fixture_path(relative: &str) -> PathBuf {
    repo_root().join(relative)
}

/// Read a repository-relative fixture, panicking with the fixture name on failure.
pub fn read_fixture(relative: &str) -> Vec<u8> {
    std::fs::read(fixture_path(relative))
        .unwrap_or_else(|error| panic!("fixture {relative}: {error}"))
}
