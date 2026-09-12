// SPDX-License-Identifier: MIT

//! Shared fixture access for `context-service` integration tests.
//!
//! Each integration test target compiles this module independently, so helpers that a given
//! target does not use would otherwise be reported as dead code.

#![allow(dead_code)]

pub mod fixtures;
