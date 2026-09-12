// SPDX-License-Identifier: MIT

//! Additive `integrated-demo` binary: a thin wrapper over the library entry point.

use std::env;

fn main() {
    if let Err(error) = run() {
        eprintln!("context console: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let port = env::args()
        .nth(1)
        .map(|value| value.parse::<u16>().map_err(|_| "invalid port".to_owned()))
        .transpose()?
        .unwrap_or(0);
    context_service::run_integrated_demo(port)
}
