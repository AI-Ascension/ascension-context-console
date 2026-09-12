// SPDX-License-Identifier: MIT

use crate::private_store::{PolicyApproval, PrivateScope, PrivateStoreError};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_PRIVATE_OBJECT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_PRIVATE_QUOTA_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncryptedContentMetadata {
    pub content_ref: String,
    pub plaintext_bytes: usize,
    pub ciphertext_bytes: usize,
    pub algorithm: &'static str,
}

#[derive(Clone)]
pub(super) struct EncryptedObject {
    nonce: [u8; 24],
    pub(super) ciphertext: Vec<u8>,
}

pub struct PrivateVault {
    key: [u8; 32],
    quota_bytes: usize,
    used_bytes: usize,
    counter: u64,
    pub(super) objects: BTreeMap<String, EncryptedObject>,
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
        scope: &PrivateScope,
        plaintext: &[u8],
    ) -> Result<EncryptedContentMetadata, PrivateStoreError> {
        if plaintext.len() > MAX_PRIVATE_OBJECT_BYTES {
            return Err(PrivateStoreError::TooLarge);
        }
        let storage_key = scope.storage_key();
        let old = self
            .objects
            .get(&storage_key)
            .map(|object| object.ciphertext.len());
        let projected = self
            .used_bytes
            .saturating_sub(old.unwrap_or(0))
            .saturating_add(plaintext.len())
            .saturating_add(16);
        if projected > self.quota_bytes {
            return Err(PrivateStoreError::Quota);
        }
        let nonce = self.next_nonce(scope, plaintext);
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.key));
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &scope.aad(),
                },
            )
            .map_err(|_| PrivateStoreError::AuthenticationFailed)?;
        let plaintext_bytes = plaintext.len();
        self.used_bytes = self
            .used_bytes
            .saturating_sub(old.unwrap_or(0))
            .saturating_add(ciphertext.len());
        self.objects.insert(
            storage_key,
            EncryptedObject {
                nonce,
                ciphertext: ciphertext.clone(),
            },
        );
        Ok(EncryptedContentMetadata {
            content_ref: scope.content_ref().to_owned(),
            plaintext_bytes,
            ciphertext_bytes: ciphertext.len(),
            algorithm: "XChaCha20-Poly1305",
        })
    }

    pub fn get(
        &self,
        scope: &PrivateScope,
        restricted_authorization: bool,
    ) -> Result<Vec<u8>, PrivateStoreError> {
        if !restricted_authorization {
            return Err(PrivateStoreError::Unauthorized);
        }
        let object = self
            .objects
            .get(&scope.storage_key())
            .ok_or(PrivateStoreError::NotFound)?;
        let cipher = XChaCha20Poly1305::new(Key::from_slice(&self.key));
        cipher
            .decrypt(
                XNonce::from_slice(&object.nonce),
                Payload {
                    msg: object.ciphertext.as_ref(),
                    aad: &scope.aad(),
                },
            )
            .map_err(|_| PrivateStoreError::AuthenticationFailed)
    }

    pub fn ciphertext(&self, scope: &PrivateScope) -> Result<&[u8], PrivateStoreError> {
        self.objects
            .get(&scope.storage_key())
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

    fn next_nonce(&mut self, scope: &PrivateScope, plaintext: &[u8]) -> [u8; 24] {
        let counter = self.counter;
        self.counter = self.counter.wrapping_add(1);
        let mut hasher = Sha256::new();
        hasher.update(self.key);
        hasher.update(counter.to_le_bytes());
        hasher.update(scope.aad());
        hasher.update(plaintext.len().to_le_bytes());
        let digest = hasher.finalize();
        let mut nonce = [0_u8; 24];
        nonce.copy_from_slice(&digest[..24]);
        nonce
    }
}
