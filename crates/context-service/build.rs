// SPDX-License-Identifier: MIT

//! Derive the pinned producer revision from the fixture this crate serves, and check the contract
//! bytes against the schemas this crate parses with.
//!
//! `PRODUCER_REVISION` and the pinned `producer.json` are two views of one fact: the revision
//! describes the harness that generated the bytes. Repeating it as a literal meant moving the
//! generator's pin silently left the served constant claiming the old revision, with nothing in
//! this repository objecting. Reading it back here makes a mismatch a build failure instead.
//!
//! The same applies to the copied contract artifacts: the fixture generator emits a v4
//! provider-session descriptor, so the bytes in `contracts/` have to be the v4 ones this crate
//! actually parses. Comparing them at build time catches a half-finished cutover, which is the
//! state the consumer-conformance gate exists to catch but cannot catch before the bytes ship.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let fixture_path = manifest_dir.join("../../fixtures/effective-limits/producer.json");
    let contracts = manifest_dir.join("../../contracts");

    println!("cargo:rerun-if-changed={}", fixture_path.display());
    for name in [
        "provider-session/capabilities.schema.json",
        "provider-session/policy.schema.json",
    ] {
        println!("cargo:rerun-if-changed={}", contracts.join(name).display());
    }

    let fixture = read(&fixture_path);

    let revision = json_string_field(&fixture, "producer_revision").unwrap_or_else(|| {
        panic!(
            "{} records no producer_revision, so the served constant cannot describe it",
            fixture_path.display()
        )
    });
    println!("cargo:rustc-env=CONSOLE_FIXTURE_PRODUCER_REVISION={revision}");

    // The fixture is the cutover's witness: if the served descriptors still carry the pre-rename
    // field while the fixture carries the post-rename one, this crate would be parsing one
    // contract and serving another. Fail here rather than at the first real request.
    let session_schema = read(&contracts.join("provider-session/capabilities.schema.json"));
    assert!(
        session_schema.contains("\"provenance\""),
        "the served provider-session schema has no `provenance` field, but the pinned fixture's descriptors do; the v4 cutover is half-applied"
    );
    assert!(
        !session_schema.contains("\"evidence\""),
        "the served provider-session schema still declares `evidence`, so this crate and the pinned fixture disagree about the contract"
    );
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()))
}

/// Extract a `"key": "value"` string from fixture bytes without a JSON dependency.
fn json_string_field(document: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let after_key = document.find(&needle)? + needle.len();
    let rest = &document[after_key..];
    let colon = rest.find(':')? + 1;
    let value = rest[colon..].trim_start();
    let value = value.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(value[..end].to_owned())
}
