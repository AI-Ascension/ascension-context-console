// SPDX-License-Identifier: MIT

//! Opt-in authenticated encrypted content retention.  The vault is deliberately a small value
//! store: callers provide an already-authorized opaque reference and never a filesystem path or
//! URL.  It refuses unsafe policy setup instead of falling back to plaintext.

mod approval;
mod error;
mod scope;
#[cfg(test)]
mod tests;
mod vault;

pub use approval::PolicyApproval;
pub use error::PrivateStoreError;
pub use scope::PrivateScope;
pub use vault::{EncryptedContentMetadata, PrivateVault};
