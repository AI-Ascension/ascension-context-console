// SPDX-License-Identifier: MIT

use std::env;
use std::fs;
use std::io::{self, Read};

use context_reader::Snapshot;

fn main() {
    if let Err(error) = run() {
        eprintln!("context snapshot rejected: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let bytes = match env::args().nth(1) {
        Some(path) => fs::read(path).map_err(|_| String::from("cannot read snapshot path"))?,
        None => {
            let mut bytes = Vec::new();
            io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|_| String::from("cannot read snapshot stdin"))?;
            bytes
        }
    };
    let snapshot = Snapshot::parse(&bytes).map_err(|error| error.to_string())?;
    let projection = snapshot.projection();
    println!("snapshot_id={}", projection.snapshot_id);
    println!("boundary={}", projection.boundary);
    println!("capture_mode={}", projection.capture_mode.as_str());
    println!(
        "application_capture_complete={}",
        projection.application_capture_complete
    );
    println!("component_count={}", projection.component_count);
    println!("provider_model={}", projection.provider_model);
    println!(
        "input_measurement_source={}",
        projection.input_measurement.source
    );
    Ok(())
}
