//! Encrypted, exact-request reservation and recovery journal for the future Console18 adapter.
//!
//! This module is intentionally not wired to an HTTP listener or transport. All admission and
//! key-provider construction is crate-private; permits expose only a validated typed Harness
//! operation, its closed endpoint, the exact typed body bytes, and the opaque protected reference.

#[path = "owner_invocation_store/crypto.rs"]
mod crypto;
#[path = "owner_invocation_store/engine.rs"]
mod engine;
#[path = "owner_invocation_store/engine_helpers.rs"]
mod engine_helpers;
#[path = "owner_invocation_store/operations.rs"]
mod operations;
#[path = "owner_invocation_store/record.rs"]
mod record;
#[path = "owner_invocation_store/record_codec.rs"]
mod record_codec;
#[path = "owner_invocation_store/storage.rs"]
mod storage;
#[path = "owner_invocation_store/transitions.rs"]
mod transitions;

pub(crate) use operations::{
    lookup_invocation, required_console_permissions, required_harness_scopes,
};

#[cfg(test)]
#[path = "owner_invocation_store/tests.rs"]
mod tests;

pub(crate) use crypto::{OwnerInvocationKeyMaterial, OwnerInvocationKeyProvider};
pub(crate) use engine::{
    ExactReservation, OneUseLookupPermit, OneUseSendPermit, OwnerInvocationStore,
    ReservationOutcome,
};
pub(crate) use record::{AdmissionError, AdmissionUse, TrustedInvocationAdmission};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum StoreError {
    Invalid,
    Denied,
    Conflict,
    UnsupportedOperation,
    KeyUnavailable,
    IndexKeyRotationRequiresOfflineMigration,
    StoreCorrupt,
    StorageLimit,
    Capacity,
    StoreUnavailable,
    CryptoUnavailable,
    UnsupportedPlatform,
    RecoveryRequired,
    AmbiguousWithoutExactLookup,
    AttemptLimit,
    InvalidOwnerResponse,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "owner invocation reservation is invalid",
            Self::Denied => "owner invocation is not authorized",
            Self::Conflict => "owner invocation key conflicts with its exact request",
            Self::UnsupportedOperation => "owner invocation operation is not journaled",
            Self::KeyUnavailable => "owner invocation store key is unavailable",
            Self::IndexKeyRotationRequiresOfflineMigration => {
                "owner invocation index key requires offline migration"
            }
            Self::StoreCorrupt => "owner invocation store is invalid",
            Self::StorageLimit => "owner invocation store reached its storage bound",
            Self::Capacity => "owner invocation store is full",
            Self::StoreUnavailable => "owner invocation store is unavailable",
            Self::CryptoUnavailable => "owner invocation encryption is unavailable",
            Self::UnsupportedPlatform => {
                "owner invocation storage security is unsupported on this platform"
            }
            Self::RecoveryRequired => "owner invocation requires exact receipt recovery",
            Self::AmbiguousWithoutExactLookup => {
                "owner invocation has no exact receipt lookup and cannot be repeated"
            }
            Self::AttemptLimit => "owner invocation recovery attempt limit was reached",
            Self::InvalidOwnerResponse => "owner response did not match the exact invocation",
        })
    }
}

impl std::fmt::Debug for StoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for StoreError {}
