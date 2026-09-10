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

/// Authorization and identity for one private component. Every field is included in the
/// authenticated associated data, so a ciphertext cannot be moved between projects, snapshots,
/// components, or opaque references and still decrypt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrivateScope {
    project_id: String,
    snapshot_id: String,
    component_id: String,
    content_ref: String,
}

impl PrivateScope {
    pub fn new(
        project_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        component_id: impl Into<String>,
        content_ref: impl Into<String>,
    ) -> Result<Self, PrivateStoreError> {
        let scope = Self {
            project_id: project_id.into(),
            snapshot_id: snapshot_id.into(),
            component_id: component_id.into(),
            content_ref: content_ref.into(),
        };
        if [
            &scope.project_id,
            &scope.snapshot_id,
            &scope.component_id,
            &scope.content_ref,
        ]
        .iter()
        .any(|value| !valid_ref(value))
        {
            return Err(PrivateStoreError::InvalidReference);
        }
        Ok(scope)
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    pub fn component_id(&self) -> &str {
        &self.component_id
    }

    pub fn content_ref(&self) -> &str {
        &self.content_ref
    }

    fn aad(&self) -> Vec<u8> {
        let mut aad = Vec::with_capacity(64 + self.content_ref.len());
        aad.extend_from_slice(b"ascension.private-context.v1\0");
        for value in [
            self.project_id.as_str(),
            self.snapshot_id.as_str(),
            self.component_id.as_str(),
            self.content_ref.as_str(),
        ] {
            aad.extend_from_slice(&(value.len() as u64).to_be_bytes());
            aad.extend_from_slice(value.as_bytes());
        }
        aad
    }

    fn storage_key(&self) -> String {
        let digest: [u8; 32] = Sha256::digest(self.aad()).into();
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
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

    fn scope(content_ref: &str) -> PrivateScope {
        PrivateScope::new(
            "agent-private",
            "snapshot-private",
            "component-private",
            content_ref,
        )
        .expect("scope")
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
        let scope = scope("blob-1");
        vault.put(&scope, b"private synthetic marker").expect("put");
        assert_ne!(
            vault.ciphertext(&scope).expect("ciphertext"),
            b"private synthetic marker"
        );
        assert_eq!(
            vault.get(&scope, true).expect("get"),
            b"private synthetic marker"
        );
        assert!(matches!(
            vault.get(&scope, false),
            Err(PrivateStoreError::Unauthorized)
        ));
        let object = vault.objects.get_mut(&scope.storage_key()).expect("object");
        object.ciphertext[0] ^= 1;
        assert!(matches!(
            vault.get(&scope, true),
            Err(PrivateStoreError::AuthenticationFailed)
        ));
    }

    #[test]
    fn ciphertext_is_bound_to_its_content_reference() {
        let mut vault = vault();
        let first = scope("blob-1");
        let second = scope("blob-2");
        vault.put(&first, b"private synthetic marker").expect("put");
        let object = vault.objects.remove(&first.storage_key()).expect("object");
        vault.objects.insert(second.storage_key(), object);
        assert!(matches!(
            vault.get(&second, true),
            Err(PrivateStoreError::AuthenticationFailed)
        ));
    }

    #[test]
    fn private_scope_binds_project_snapshot_and_component_identity() {
        let mut vault = vault();
        let original = scope("blob-1");
        vault
            .put(&original, b"private synthetic marker")
            .expect("put");
        let moved = PrivateScope::new(
            "other-project",
            original.snapshot_id(),
            original.component_id(),
            original.content_ref(),
        )
        .expect("scope");
        assert!(matches!(
            vault.get(&moved, true),
            Err(PrivateStoreError::NotFound)
        ));
    }
}
