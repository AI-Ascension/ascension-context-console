// SPDX-License-Identifier: MIT

use context_service::demo;
use std::env;
use std::fs;
use std::io::{self, Read};

fn main() {
    if let Err(error) = run() {
        eprintln!("context console: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("demo") => demo().map_err(|error| error.to_string()),
        Some("inspect") => {
            let bytes = match arguments.next() {
                Some(path) => fs::read(path).map_err(|_| "cannot read snapshot path".to_owned())?,
                None => {
                    let mut bytes = Vec::new();
                    io::stdin()
                        .read_to_end(&mut bytes)
                        .map_err(|_| "cannot read snapshot stdin".to_owned())?;
                    bytes
                }
            };
            let snapshot =
                context_reader::Snapshot::parse(&bytes).map_err(|error| error.to_string())?;
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
        Some("health") | None => {
            println!("{{\"status\":\"ok\",\"read_only\":true}}");
            Ok(())
        }
        Some("help") => {
            println!("context-console health|demo|inspect [snapshot.json]");
            Ok(())
        }
        Some(command) => Err(format!("unsupported command: {command}")),
    }
}
