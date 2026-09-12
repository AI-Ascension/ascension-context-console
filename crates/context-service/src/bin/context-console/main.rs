// SPDX-License-Identifier: MIT

//! Thin entry point for the `context-console` CLI.

mod cli;
mod demo;
mod inspect;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("context console: {error}");
        std::process::exit(2);
    }
}
