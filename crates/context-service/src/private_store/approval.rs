// SPDX-License-Identifier: MIT

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyApproval {
    pub accepted: bool,
    pub restricted_authorization: bool,
}
