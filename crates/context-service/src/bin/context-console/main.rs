// SPDX-License-Identifier: MIT

//! Thin entry point for the `context-console` CLI.

mod cli;
mod demo;
mod inspect;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("context console: {error}");
        std::process::exit(error_exit_code(&error));
    }
}

fn error_exit_code(error: &str) -> i32 {
    if [
        "stale_",
        "conflict",
        "expired_",
        "already_paused",
        "run_not_ready",
        "not_ready",
        "preview_stale",
    ]
    .iter()
    .any(|marker| error.contains(marker))
    {
        return 3;
    }
    if [
        "authentication",
        "permission",
        "forbidden",
        "protected_item",
        "objective",
        "content_permission",
        "stopped",
    ]
    .iter()
    .any(|marker| error.contains(marker))
    {
        return 4;
    }
    if error.contains("durable control")
        || error.contains("database operation")
        || error.contains("journal is unavailable")
    {
        return 5;
    }
    2
}
