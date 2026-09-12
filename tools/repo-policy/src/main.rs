// SPDX-License-Identifier: MIT

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    repo_policy::run(&arguments)
}
