// SPDX-License-Identifier: MIT

//! LANG001: Python source or package metadata is prohibited.

use std::fs;
use std::path::Path;

/// Walk `root` for Python sources or package metadata, skipping `.git` and `target`.
pub fn contains_python(root: &Path) -> bool {
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

pub fn violations(root: &Path) -> Vec<String> {
    if contains_python(root) {
        vec!["LANG001 Python source or package metadata is prohibited".to_owned()]
    } else {
        Vec::new()
    }
}
