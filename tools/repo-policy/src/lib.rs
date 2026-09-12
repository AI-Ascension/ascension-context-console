// SPDX-License-Identifier: MIT

//! Read-only policy gate for the Context Console repository.
//!
//! The binary keeps the historical command surface: it scans the repository root
//! for required files, required directories, and prohibited Python sources.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub mod rules;

#[cfg(test)]
mod tests;

/// Resolve the repository root from the arguments that follow the program name.
///
/// The first argument that is not `--strict` is treated as the root; otherwise the
/// current working directory is used, falling back to `.` when it cannot be read.
pub fn resolve_root(arguments: &[String]) -> PathBuf {
    arguments
        .iter()
        .find(|argument| argument.as_str() != "--strict")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Collect and print every violation for `root`, returning the violation count.
pub fn check(root: &Path) -> u32 {
    let violations = rules::evaluate(root);
    for violation in &violations {
        eprintln!("{violation}");
    }
    violations.len() as u32
}

/// Run the policy gate and translate the violation count into an exit code.
pub fn run(arguments: &[String]) -> ExitCode {
    let root = resolve_root(arguments);
    let errors = check(&root);
    if errors == 0 {
        println!("Policy check: required files and language boundaries passed");
        ExitCode::SUCCESS
    } else {
        eprintln!("Policy check: {errors} error(s)");
        ExitCode::FAILURE
    }
}
