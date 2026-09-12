// SPDX-License-Identifier: MIT

//! Focused tests for the repository policy rules.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::rules::{language, layout, required_files};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A disposable repository-shaped directory under the system temp dir.
struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Self {
        let unique = format!(
            "repo-policy-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&root).expect("create temporary tree");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_file(root: &Path, relative: &str, contents: &[u8]) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent directory");
    }
    fs::write(path, contents).expect("write test file");
}

#[test]
fn required_file_list_is_unchanged() {
    const EXPECTED: &[&str] = &[
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
    assert_eq!(required_files::REQUIRED, EXPECTED);
}

#[test]
fn missing_required_files_are_reported() {
    let tree = TempTree::new();
    let violations = required_files::violations(tree.path());
    assert_eq!(violations.len(), required_files::REQUIRED.len());
    assert!(
        violations
            .iter()
            .any(|message| message == "DOC001 missing required file: README.md")
    );
}

#[test]
fn present_required_files_pass() {
    let tree = TempTree::new();
    for relative in required_files::REQUIRED {
        write_file(tree.path(), relative, b"fixture\n");
    }
    assert!(required_files::violations(tree.path()).is_empty());
}

#[test]
fn missing_directories_are_reported_in_order() {
    let tree = TempTree::new();
    assert_eq!(
        layout::violations(tree.path()),
        vec![
            "LAYOUT001 missing directory: crates".to_owned(),
            "LAYOUT001 missing directory: tools".to_owned(),
        ]
    );
}

#[test]
fn present_directories_pass() {
    let tree = TempTree::new();
    fs::create_dir_all(tree.path().join("crates")).expect("create crates directory");
    fs::create_dir_all(tree.path().join("tools")).expect("create tools directory");
    assert!(layout::violations(tree.path()).is_empty());
}

#[test]
fn python_sources_and_metadata_are_detected() {
    let source = TempTree::new();
    write_file(source.path(), "module.py", b"pass\n");
    assert!(language::contains_python(source.path()));

    let stub = TempTree::new();
    write_file(stub.path(), "module.pyi", b"pass\n");
    assert!(language::contains_python(stub.path()));

    let metadata = TempTree::new();
    write_file(metadata.path(), "pyproject.toml", b"[project]\n");
    assert!(language::contains_python(metadata.path()));
}

#[test]
fn ignored_directories_are_skipped() {
    let tree = TempTree::new();
    write_file(tree.path(), "target/module.py", b"pass\n");
    write_file(tree.path(), ".git/module.pyi", b"pass\n");
    assert!(!language::contains_python(tree.path()));
}

#[test]
fn evaluate_reports_files_then_directories_then_language() {
    let tree = TempTree::new();
    write_file(tree.path(), "module.py", b"pass\n");
    let violations = crate::rules::evaluate(tree.path());
    assert_eq!(violations.len(), required_files::REQUIRED.len() + 2 + 1);
    assert!(violations[0].starts_with("DOC001"));
    assert_eq!(
        violations[required_files::REQUIRED.len()],
        "LAYOUT001 missing directory: crates"
    );
    assert_eq!(
        violations.last().map(String::as_str),
        Some("LANG001 Python source or package metadata is prohibited")
    );
}
