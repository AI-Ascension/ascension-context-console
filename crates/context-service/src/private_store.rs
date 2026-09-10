// SPDX-License-Identifier: MIT

//! Opt-in authenticated encrypted content retention.  The vault is deliberately a small value
//! store: callers provide an already-authorized opaque reference and never a filesystem path or
//! URL.  It refuses unsafe policy setup instead of falling back to plaintext.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_PRIVATE_OBJECT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PRIVATE_QUOTA_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PolicyApproval {
    pub accepted: bool,
    pub restricted_authorization: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrivateStoreError {
    PolicyNotApproved,
    InvalidKey,
    InvalidReference,
    TooLarge,
    Quota,
    NotFound,
    Unauthorized,
    AuthenticationFailed,
}

impl std::fmt::Display for PrivateStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::PolicyNotApproved => "private capture policy is not approved",
            Self::InvalidKey => "private capture key is invalid",
            Self::InvalidReference => "content reference is invalid",
            Self::TooLarge => "private content exceeds its bound",
            Self::Quota => "private content quota is full",
            Self::NotFound => "private content is unavailable",
            Self::Unauthorized => "private content authorization is missing",
            Self::AuthenticationFailed => "private content authentication failed",
        })
    }
}

impl std::error::Error for PrivateStoreError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedContentMetadata {
    pub content_ref: String,
    pub plaintext_bytes: usize,
    pub ciphertext_bytes: usize,
    pub algorithm: &'static str,
}

#[derive(Clone)]
struct EncryptedObject {
    nonce: [u8; 24],
    ciphertext: Vec<u8>,
}

pub struct PrivateVault {
    key: [u8; 32],
    quota_bytes: usize,
    used_bytes: usize,
    counter: u64,
    objects: BTreeMap<String, EncryptedObject>,
}

impl PrivateVault {
    pub fn new(
        key: [u8; 32],
        quota_bytes: usize,
        approval: PolicyApproval,
    ) -> Result<Self, PrivateStoreError> {
        if !approval.accepted || !approval.restricted_authorization {
            return Err(PrivateStoreError::PolicyNotApproved);
        }
        if key.iter().all(|byte| *byte == 0) {
            return Err(PrivateStoreError::InvalidKey);
        }
        if quota_bytes == 0 || quota_bytes > MAX_PRIVATE_QUOTA_BYTES {
            return Err(PrivateStoreError::Quota);
        }
        Ok(Self {
            key,
            quota_bytes,
            used_bytes: 0,
            counter: 0,
            objects: BTreeMap::new(),
        })
    }

    pub fn put(
        &mut self,
        content_ref: &str,
        plaintext: &[u8],
    ) -> Result<EncryptedContentMetadata, PrivateStoreError> {
        if !valid_ref(content_ref) {
            return Err(PrivateStoreError::InvalidReference);
        }
        if plaintext.len() > MAX_PRIVATE_OBJECT_BYTES {
            return Err(PrivateStoreError::TooLarge);
        }
        let old = self
            .objects
            .get(content_ref)
            .map(|object| object.ciphertext.len());
        let projected = self
            .used_bytes
            .saturating_sub(old.unwrap_or(0))
            .saturating_add(plaintext.len())
            .saturating_add(16);
        if projected > self.quota_bytes {
            return Err(PrivateStoreError::Quota);
        }
        let nonce = self.next_nonce(content_ref, plaintext);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.key));
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: content_ref.as_bytes(),
                },
            )
            .map_err(|_| PrivateStoreError::AuthenticationFailed)?;
        let plaintext_bytes = plaintext.len();
        self.used_bytes = self
            .used_bytes
            .saturating_sub(old.unwrap_or(0))
            .saturating_add(ciphertext.len());
        self.objects.insert(
            content_ref.to_owned(),
            EncryptedObject {
                nonce,
                ciphertext: ciphertext.clone(),
            },
        );
        Ok(EncryptedContentMetadata {
            content_ref: content_ref.to_owned(),
            plaintext_bytes,
            ciphertext_bytes: ciphertext.len(),
            algorithm: "XChaCha20-Poly1305",
        })
    }

    pub fn get(
        &self,
        content_ref: &str,
        restricted_authorization: bool,
    ) -> Result<Vec<u8>, PrivateStoreError> {
        if !restricted_authorization {
            return Err(PrivateStoreError::Unauthorized);
        }
        let object = self
            .objects
            .get(content_ref)
            .ok_or(PrivateStoreError::NotFound)?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.key));
        cipher
            .decrypt(
                XNonce::from_slice(&object.nonce),
                Payload {
                    msg: object.ciphertext.as_ref(),
                    aad: content_ref.as_bytes(),
                },
            )
            .map_err(|_| PrivateStoreError::AuthenticationFailed)
    }

    pub fn ciphertext(&self, content_ref: &str) -> Result<&[u8], PrivateStoreError> {
        self.objects
            .get(content_ref)
            .map(|object| object.ciphertext.as_slice())
            .ok_or(PrivateStoreError::NotFound)
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    fn next_nonce(&mut self, content_ref: &str, plaintext: &[u8]) -> [u8; 24] {
        let counter = self.counter;
        self.counter = self.counter.wrapping_add(1);
        let mut hasher = Sha256::new();
        hasher.update(self.key);
        hasher.update(counter.to_le_bytes());
        hasher.update(content_ref.as_bytes());
        hasher.update(plaintext.len().to_le_bytes());
        let digest = hasher.finalize();
        let mut nonce = [0_u8; 24];
        nonce.copy_from_slice(&digest[..24]);
        nonce
    }
}

fn valid_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> PrivateVault {
        PrivateVault::new(
            [7_u8; 32],
            1024,
            PolicyApproval {
                accepted: true,
                restricted_authorization: true,
            },
        )
        .expect("vault")
    }

    #[test]
    fn policy_never_downgrades_to_plaintext() {
        assert!(matches!(
            PrivateVault::new(
                [7_u8; 32],
                1024,
                PolicyApproval {
                    accepted: false,
                    restricted_authorization: true,
                }
            ),
            Err(PrivateStoreError::PolicyNotApproved)
        ));
    }

    #[test]
    fn encrypted_content_round_trips_and_tampering_fails() {
        let mut vault = vault();
        vault
            .put("blob-1", b"private synthetic marker")
            .expect("put");
        assert_ne!(
            vault.ciphertext("blob-1").expect("ciphertext"),
            b"private synthetic marker"
        );
        assert_eq!(
            vault.get("blob-1", true).expect("get"),
            b"private synthetic marker"
        );
        assert!(matches!(
            vault.get("blob-1", false),
            Err(PrivateStoreError::Unauthorized)
        ));
        let object = vault.objects.get_mut("blob-1").expect("object");
        object.ciphertext[0] ^= 1;
        assert!(matches!(
            vault.get("blob-1", true),
            Err(PrivateStoreError::AuthenticationFailed)
        ));
    }

    #[test]
    fn ciphertext_is_bound_to_its_content_reference() {
        let mut vault = vault();
        vault
            .put("blob-1", b"private synthetic marker")
            .expect("put");
        let object = vault.objects.remove("blob-1").expect("object");
        vault.objects.insert("blob-2".to_owned(), object);
        assert!(matches!(
            vault.get("blob-2", true),
            Err(PrivateStoreError::AuthenticationFailed)
        ));
    }
}
