use hmac::{Hmac, KeyInit, Mac};
use owner_seed_sha2::Sha256;
use std::collections::BTreeMap;
use zeroize::Zeroizing;

use super::StoreError;
use super::record::StableLocator;
use super::storage::StoredRow;

type HmacSha256 = Hmac<Sha256>;
const AAD_DOMAIN: &[u8] = b"ascension.console18.owner-invocation-store.v1\0";
const INDEX_DOMAIN: &[u8] = b"ascension.console18.owner-invocation-index.v1\0";
const INDEX_KEY_CHECK_DOMAIN: &[u8] = b"ascension.console18.owner-invocation-index-key-check.v2\0";

/// Keys are injected by an operator-owned provider. There is deliberately no environment,
/// file-format, or production-key configuration in this module.
pub(crate) trait OwnerInvocationKeyProvider {
    fn load(&mut self) -> Result<OwnerInvocationKeyMaterial, StoreError>;
}

/// Opaque key material. It cannot be formatted or serialized, and buffers zeroize on drop.
pub(crate) struct OwnerInvocationKeyMaterial {
    index_key_id: String,
    index_key: Zeroizing<[u8; 32]>,
    current_data_key_id: String,
    data_keys: BTreeMap<String, Zeroizing<[u8; 32]>>,
}

impl OwnerInvocationKeyMaterial {
    pub(crate) fn new(
        index_key_id: String,
        index_key: [u8; 32],
        current_data_key_id: String,
        data_keys: BTreeMap<String, [u8; 32]>,
    ) -> Result<Self, StoreError> {
        let index_key = Zeroizing::new(index_key);
        let data_keys = data_keys
            .into_iter()
            .map(|(id, key)| (id, Zeroizing::new(key)))
            .collect::<BTreeMap<_, _>>();
        let mut prior_keys: Vec<&[u8; 32]> = Vec::with_capacity(data_keys.len());
        let mut duplicate_data_key = false;
        for key in data_keys.values() {
            let key_bytes: &[u8; 32] = key;
            if prior_keys.contains(&key_bytes) {
                duplicate_data_key = true;
                break;
            }
            prior_keys.push(key_bytes);
        }
        if !valid_key_id(&index_key_id)
            || !valid_key_id(&current_data_key_id)
            || data_keys.is_empty()
            || data_keys.len() > 8
            || !data_keys.contains_key(&current_data_key_id)
            || data_keys.keys().any(|key_id| !valid_key_id(key_id))
            || data_keys
                .values()
                .any(|key| key.as_ref() == index_key.as_ref())
            || duplicate_data_key
        {
            return Err(StoreError::KeyUnavailable);
        }
        Ok(Self {
            index_key_id,
            index_key,
            current_data_key_id,
            data_keys,
        })
    }

    pub(super) fn index_key_id(&self) -> &str {
        &self.index_key_id
    }

    pub(super) fn current_data_key_id(&self) -> &str {
        &self.current_data_key_id
    }

    pub(super) fn index_key(&self) -> &[u8; 32] {
        &self.index_key
    }

    pub(super) fn data_key(&self, key_id: &str) -> Option<&[u8; 32]> {
        self.data_keys.get(key_id).map(|key| &**key)
    }

    pub(super) fn same_index_material(&self, other: &Self) -> bool {
        self.index_key_id == other.index_key_id
            && self.index_key.as_ref() == other.index_key.as_ref()
    }

    pub(super) fn retains_existing_data_key_material(&self, other: &Self) -> bool {
        self.data_keys.iter().all(|(key_id, key)| {
            other
                .data_key(key_id)
                .is_some_and(|other_key| key.as_ref() == other_key)
        })
    }
}

impl std::fmt::Debug for OwnerInvocationKeyMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OwnerInvocationKeyMaterial(<redacted>)")
    }
}

pub(super) fn lookup_tag(key: &[u8; 32], locator: &StableLocator) -> Result<[u8; 32], StoreError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| StoreError::KeyUnavailable)?;
    mac.update(INDEX_DOMAIN);
    for field in [
        locator.issuer.as_bytes(),
        locator.subject.as_bytes(),
        locator.audience.as_bytes(),
        locator.scope.project_id.as_bytes(),
        locator.scope.run_id.as_bytes(),
        locator.scope.episode_id.as_bytes(),
        locator.scope.agent_id.as_bytes(),
        locator.family.as_str().as_bytes(),
        locator.stable_key.as_bytes(),
    ] {
        update_length_prefixed(&mut mac, field)?;
    }
    Ok(mac.finalize().into_bytes().into())
}

pub(super) fn index_key_verifier(key: &[u8; 32], key_id: &str) -> Result<[u8; 32], StoreError> {
    let mac = index_key_check_mac(key, key_id)?;
    Ok(mac.finalize().into_bytes().into())
}

pub(super) fn verify_index_key(
    key: &[u8; 32],
    key_id: &str,
    verifier: &[u8],
) -> Result<(), StoreError> {
    if verifier.len() != 32 {
        return Err(StoreError::StoreCorrupt);
    }
    index_key_check_mac(key, key_id)?
        .verify_slice(verifier)
        .map_err(|_| StoreError::KeyUnavailable)
}

fn index_key_check_mac(key: &[u8; 32], key_id: &str) -> Result<HmacSha256, StoreError> {
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| StoreError::KeyUnavailable)?;
    mac.update(INDEX_KEY_CHECK_DOMAIN);
    mac.update(&2_u16.to_be_bytes());
    update_length_prefixed(&mut mac, key_id.as_bytes())?;
    Ok(mac)
}

pub(super) fn encrypt(
    keys: &OwnerInvocationKeyMaterial,
    tag: &[u8; 32],
    entry_id: &[u8; 16],
    state: i64,
    sequence: u64,
    plaintext: &[u8],
) -> Result<([u8; 24], Vec<u8>), StoreError> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

    let key_id = keys.current_data_key_id();
    let key = keys.data_key(key_id).ok_or(StoreError::KeyUnavailable)?;
    let mut nonce = [0_u8; 24];
    getrandom::getrandom(&mut nonce).map_err(|_| StoreError::CryptoUnavailable)?;
    let aad = associated_data(keys.index_key_id(), tag, entry_id, state, sequence, key_id)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| StoreError::CryptoUnavailable)?;
    Ok((nonce, ciphertext))
}

pub(super) fn decrypt(
    keys: &OwnerInvocationKeyMaterial,
    index_key_id: &str,
    row: &StoredRow,
) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

    if index_key_id != keys.index_key_id() || row.ciphertext.is_empty() {
        return Err(StoreError::StoreCorrupt);
    }
    let key = keys
        .data_key(&row.data_key_id)
        .ok_or(StoreError::KeyUnavailable)?;
    let aad = associated_data(
        index_key_id,
        &row.tag,
        &row.entry_id,
        row.state as i64,
        row.sequence,
        &row.data_key_id,
    )?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&row.nonce),
            Payload {
                msg: &row.ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| StoreError::StoreCorrupt)?;
    Ok(Zeroizing::new(plaintext))
}

pub(super) fn associated_data(
    index_key_id: &str,
    tag: &[u8; 32],
    entry_id: &[u8; 16],
    state: i64,
    sequence: u64,
    data_key_id: &str,
) -> Result<Vec<u8>, StoreError> {
    let state = u8::try_from(state).map_err(|_| StoreError::StoreCorrupt)?;
    let mut aad = AAD_DOMAIN.to_vec();
    aad.extend_from_slice(&1_u16.to_be_bytes());
    append_length_prefixed(&mut aad, index_key_id.as_bytes())?;
    aad.extend_from_slice(tag);
    aad.extend_from_slice(entry_id);
    aad.push(state);
    aad.extend_from_slice(&sequence.to_be_bytes());
    append_length_prefixed(&mut aad, data_key_id.as_bytes())?;
    Ok(aad)
}

fn update_length_prefixed(mac: &mut HmacSha256, bytes: &[u8]) -> Result<(), StoreError> {
    let len = u64::try_from(bytes.len()).map_err(|_| StoreError::Invalid)?;
    mac.update(&len.to_be_bytes());
    mac.update(bytes);
    Ok(())
}

fn append_length_prefixed(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), StoreError> {
    let len = u32::try_from(bytes.len()).map_err(|_| StoreError::Invalid)?;
    output.extend_from_slice(&len.to_be_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}
