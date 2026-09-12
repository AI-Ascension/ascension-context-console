// SPDX-License-Identifier: MIT

//! Command dispatch for the `context-console` CLI.
//!
//! The surface stays byte-compatible with the former single-file binary:
//! `health|demo|integrated-demo [port]|inspect [snapshot.json]`.

use std::env;

pub(super) fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("demo") => super::demo::run_demo(),
        Some("integrated-demo") => {
            let port = arguments
                .next()
                .map(|value| value.parse::<u16>().map_err(|_| "invalid port".to_owned()))
                .transpose()?
                .unwrap_or(0);
            super::demo::run_integrated_demo(port)
        }
        Some("inspect") => super::inspect::run(arguments.next()),
        Some("health") | None => {
            println!("{{\"status\":\"ok\",\"read_only\":true}}");
            Ok(())
        }
        Some("help") => {
            println!("context-console health|demo|integrated-demo [port]|inspect [snapshot.json]");
            Ok(())
        }
        Some(command) => Err(format!("unsupported command: {command}")),
    }
}
