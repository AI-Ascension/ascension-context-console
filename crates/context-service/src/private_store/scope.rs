// SPDX-License-Identifier: MIT

use crate::private_store::PrivateStoreError;
use sha2::{Digest, Sha256};

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

    pub(super) fn aad(&self) -> Vec<u8> {
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

    pub(super) fn storage_key(&self) -> String {
        let digest: [u8; 32] = Sha256::digest(self.aad()).into();
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

fn valid_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}
