// SPDX-License-Identifier: MIT

//! Resolve the producer revision from the lockfile so the generated fixture cannot misreport it.
//!
//! The `sts2-harness` git dependency's resolved revision lives in `Cargo.lock`, next to this
//! manifest. Reading it back at build time keeps `producer_revision` in the emitted fixture
//! correct by construction: move the pin in `Cargo.toml`, and this follows it, instead of leaving
//! a stale literal that claims the bytes came from a revision they did not.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let lockfile = manifest_dir.join("Cargo.lock");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let lock = fs::read_to_string(&lockfile)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", lockfile.display()));

    // The harness is the only `git+https://github.com/AI-Ascension/sts2-harness` source in the
    // lockfile, and each such `source` line ends in `#<rev>`. Taking the rev from the URL's fragment
    // means the value is the one cargo actually resolved, not one repeated by hand.
    let revision = lock
        .lines()
        .filter_map(|line| line.trim().strip_prefix("source = \""))
        .filter_map(|value| value.strip_suffix('"'))
        .filter(|value| value.contains("github.com/AI-Ascension/sts2-harness"))
        .filter_map(|value| value.rsplit_once('#').map(|(_, rev)| rev))
        .next()
        .unwrap_or_else(|| {
            panic!(
                "{} records no resolved sts2-harness git source, so the producer revision cannot be \
                 derived; refusing to emit a fixture that would misreport its own provenance",
                lockfile.display()
            )
        });

    // The same rev is expected in the manifest's pin. Disagreement means the lockfile and the
    // manifest have diverged, which is exactly the drift this is meant to make visible.
    let manifest = fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .unwrap_or_else(|error| panic!("Cargo.toml must be readable: {error}"));
    assert!(
        manifest.contains(revision),
        "Cargo.lock resolves sts2-harness to {revision} but Cargo.toml does not pin that rev"
    );

    println!("cargo:rustc-env=STS2_HARNESS_PRODUCER_REVISION={revision}");
}
