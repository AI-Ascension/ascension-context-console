// SPDX-License-Identifier: MIT

//! Integrity checks for the inert, copied harness contract artifact.

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const ARTIFACT_DIRECTORY: &str = "contract-artifact/context-inspection-v1";
const MANIFEST: &str = "contract-artifact/context-inspection-v1/manifest.json";

pub fn violations(root: &Path) -> Vec<String> {
    let manifest_path = root.join(MANIFEST);
    let bytes = match fs::read(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => return vec![format!("ARTIFACT001 cannot read {MANIFEST}: {error}")],
    };
    let manifest: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(manifest) => manifest,
        Err(error) => return vec![format!("ARTIFACT001 invalid {MANIFEST}: {error}")],
    };
    let files = match manifest.get("files").and_then(serde_json::Value::as_array) {
        Some(files) if !files.is_empty() => files,
        _ => return vec![format!("ARTIFACT001 {MANIFEST} has no files")],
    };
    let mut violations = Vec::new();
    let mut paths = HashSet::new();
    for file in files {
        let Some(relative) = file.get("path").and_then(serde_json::Value::as_str) else {
            violations.push("ARTIFACT001 manifest file has no path".to_owned());
            continue;
        };
        if relative.is_empty()
            || Path::new(relative).is_absolute()
            || relative.contains('\\')
            || relative
                .split('/')
                .any(|part| part.is_empty() || part == ".." || part == ".")
        {
            violations.push(format!("ARTIFACT001 unsafe artifact path: {relative}"));
            continue;
        }
        if !paths.insert(relative) {
            violations.push(format!("ARTIFACT001 duplicate artifact path: {relative}"));
            continue;
        }
        let Some(expected) = file.get("sha256").and_then(serde_json::Value::as_str) else {
            violations.push(format!("ARTIFACT001 missing digest: {relative}"));
            continue;
        };
        if expected.len() != 64
            || !expected
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            violations.push(format!("ARTIFACT001 invalid digest: {relative}"));
            continue;
        }
        let artifact_path = root.join(ARTIFACT_DIRECTORY).join(relative);
        match fs::symlink_metadata(&artifact_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                violations.push(format!("ARTIFACT001 symlinked artifact path: {relative}"));
                continue;
            }
            Ok(metadata) if !metadata.is_file() => {
                violations.push(format!(
                    "ARTIFACT001 artifact path is not a file: {relative}"
                ));
                continue;
            }
            Ok(_) => {}
            Err(error) => {
                violations.push(format!("ARTIFACT001 cannot read {relative}: {error}"));
                continue;
            }
        }
        match fs::read(&artifact_path) {
            Ok(actual) if digest(&actual) == expected => {}
            Ok(_) => violations.push(format!("ARTIFACT001 digest mismatch: {relative}")),
            Err(error) => violations.push(format!("ARTIFACT001 cannot read {relative}: {error}")),
        }
    }
    violations
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
