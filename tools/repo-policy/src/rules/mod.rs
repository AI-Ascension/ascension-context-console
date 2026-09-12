// SPDX-License-Identifier: MIT

//! Individual policy rules and their shared evaluation order.

pub mod language;
pub mod layout;
pub mod required_files;

use std::path::Path;

/// Evaluate every rule against `root`, preserving the historical report order:
/// required files, then required directories, then language boundaries.
pub fn evaluate(root: &Path) -> Vec<String> {
    let mut violations = required_files::violations(root);
    violations.extend(layout::violations(root));
    violations.extend(language::violations(root));
    violations
}
