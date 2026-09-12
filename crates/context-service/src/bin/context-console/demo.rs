// SPDX-License-Identifier: MIT

//! `demo` and `integrated-demo` dispatch for the `context-console` CLI.

pub(super) fn run_demo() -> Result<(), String> {
    context_service::demo().map_err(|error| error.to_string())
}

pub(super) fn run_integrated_demo(port: u16) -> Result<(), String> {
    context_service::run_integrated_demo(port)
}
