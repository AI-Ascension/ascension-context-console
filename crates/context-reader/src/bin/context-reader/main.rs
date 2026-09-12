// SPDX-License-Identifier: MIT

mod cli;

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
    cli::print_projection(snapshot.projection());
    Ok(())
}
