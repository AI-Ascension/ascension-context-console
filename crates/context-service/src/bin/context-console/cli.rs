// SPDX-License-Identifier: MIT

//! Command dispatch for the `context-console` CLI.

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
        Some("phase2-demo") => super::demo::run_phase2_demo(),
        Some("phase2-cli") => context_service::run_phase2_cli(arguments.collect()),
        Some("phase3-cli") => context_service::run_phase3_cli(arguments.collect()),
        Some("phase4-cli") => context_service::run_phase4_cli(arguments.collect()),
        Some("phase3-adapter") => {
            if arguments.next().is_some() {
                return Err("phase3-adapter: unexpected argument".to_owned());
            }
            context_service::run_phase3_adapter()
        }
        Some("inspect") => super::inspect::run(arguments.next()),
        Some("health") | None => {
            println!("{{\"status\":\"ok\",\"read_only\":true}}");
            Ok(())
        }
        Some("help") => {
            println!(
                "context-console health|demo|phase2-demo|phase2-cli <command> ...|phase3-cli <command>|phase3-adapter|phase4-cli <command>|integrated-demo [port]|inspect [snapshot.json]"
            );
            Ok(())
        }
        Some(command) => Err(format!("unsupported command: {command}")),
    }
}
