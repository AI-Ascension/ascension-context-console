// SPDX-License-Identifier: MIT

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const REQUIRED: &[&str] = &[
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

fn main() -> ExitCode {
    let root = env::args()
        .skip(1)
        .find(|argument| argument != "--strict")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut errors = 0_u32;
    for relative in REQUIRED {
        if !root.join(relative).is_file() {
            eprintln!("DOC001 missing required file: {relative}");
            errors += 1;
        }
    }
    for relative in &["crates", "tools"] {
        if !root.join(relative).is_dir() {
            eprintln!("LAYOUT001 missing directory: {relative}");
            errors += 1;
        }
    }
    if contains_python(&root) {
        eprintln!("LANG001 Python source or package metadata is prohibited");
        errors += 1;
    }
    if errors == 0 {
        println!("Policy check: required files and language boundaries passed");
        ExitCode::SUCCESS
    } else {
        eprintln!("Policy check: {errors} error(s)");
        ExitCode::FAILURE
    }
}

fn contains_python(root: &Path) -> bool {
    let mut stack = vec![root.to_owned()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = fs::read_dir(path) else {
            continue;
        };
        for entry in entries.flatten() {
            let child = entry.path();
            if child
                .file_name()
                .is_some_and(|name| name == ".git" || name == "target")
            {
                continue;
            }
            if child.is_dir() {
                stack.push(child);
            } else if child
                .extension()
                .is_some_and(|extension| extension == "py" || extension == "pyi")
                || child
                    .file_name()
                    .is_some_and(|name| name == "pyproject.toml" || name == "setup.py")
            {
                return true;
            }
        }
    }
    false
}
