// SPDX-License-Identifier: MIT

//! `inspect` projection printing for the `context-console` CLI.

use std::fs;
use std::io::{self, Read};

pub(super) fn run(path: Option<String>) -> Result<(), String> {
    let bytes = match path {
        Some(path) => fs::read(path).map_err(|_| "cannot read snapshot path".to_owned())?,
        None => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|_| "cannot read snapshot stdin".to_owned())?;
            bytes
        }
    };
    let snapshot = context_reader::Snapshot::parse(&bytes).map_err(|error| error.to_string())?;
    let projection = snapshot.projection();
    println!("snapshot_id={}", projection.snapshot_id);
    println!("run_id={}", projection.identity.run_id);
    println!("boundary={}", projection.boundary);
    println!("capture_mode={}", projection.capture_mode.as_str());
    println!(
        "application_capture_complete={}",
        projection.application_capture_complete
    );
    println!("component_count={}", projection.component_count);
    Ok(())
}
