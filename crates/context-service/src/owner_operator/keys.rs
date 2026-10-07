use crate::owner_invocation_store::{
    OwnerInvocationKeyMaterial, OwnerInvocationKeyProvider, StoreError,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

use super::config::OperatorConfig;
use super::protected_files::{PrivateRoot, validate_name};

const SCHEMA: &str = "ascension.context-console.owner-journal-keys.v1";
const MAX_MANIFEST_BYTES: usize = 16 * 1024;
const MAX_DATA_KEYS: usize = 8;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: String,
    index_key_id: String,
    index_key_ref: String,
    current_data_key_id: String,
    data_keys: Vec<DataKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DataKey {
    key_id: String,
    file_ref: String,
}

pub(super) struct FileJournalKeyProvider {
    root: PrivateRoot,
    manifest_ref: String,
    owner_slots_ref: String,
    forbidden_refs: BTreeSet<String>,
}

impl FileJournalKeyProvider {
    pub(super) fn new(config: &OperatorConfig) -> Self {
        Self {
            root: config.root.clone(),
            manifest_ref: config.journal_key_manifest_ref.clone(),
            owner_slots_ref: config.owner_slots_ref.clone(),
            forbidden_refs: config.protected_file_refs(),
        }
    }
}

impl OwnerInvocationKeyProvider for FileJournalKeyProvider {
    fn load(&mut self) -> Result<OwnerInvocationKeyMaterial, StoreError> {
        let manifest = load_manifest(
            &self.root,
            &self.manifest_ref,
            &self.owner_slots_ref,
            &self.forbidden_refs,
        )?;
        let index_key = read_key(&self.root, &manifest.index_key_ref)?;
        let mut protected_data_keys = BTreeMap::new();
        for entry in &manifest.data_keys {
            let key = read_key(&self.root, &entry.file_ref)?;
            if protected_data_keys
                .insert(entry.key_id.clone(), key)
                .is_some()
            {
                return Err(StoreError::KeyUnavailable);
            }
        }
        let data_keys = protected_data_keys
            .iter()
            .map(|(id, key)| (id.clone(), **key))
            .collect();
        OwnerInvocationKeyMaterial::new(
            manifest.index_key_id,
            *index_key,
            manifest.current_data_key_id,
            data_keys,
        )
    }
}

pub(super) fn journal_key_file_refs(
    root: &PrivateRoot,
    manifest_ref: &str,
    owner_slots_ref: &str,
    forbidden_refs: &BTreeSet<String>,
) -> Result<BTreeSet<String>, StoreError> {
    let manifest = load_manifest(root, manifest_ref, owner_slots_ref, forbidden_refs)?;
    Ok(std::iter::once(manifest.index_key_ref)
        .chain(manifest.data_keys.into_iter().map(|entry| entry.file_ref))
        .collect())
}

fn load_manifest(
    root: &PrivateRoot,
    manifest_ref: &str,
    owner_slots_ref: &str,
    forbidden_refs: &BTreeSet<String>,
) -> Result<Manifest, StoreError> {
    let bytes = root
        .read_file(manifest_ref, MAX_MANIFEST_BYTES)
        .map_err(|_| StoreError::KeyUnavailable)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| StoreError::KeyUnavailable)?;
    let bearer_refs = super::slots::bearer_file_refs(root, owner_slots_ref, forbidden_refs)
        .map_err(|_| StoreError::KeyUnavailable)?;
    let mut disjoint_refs = forbidden_refs.clone();
    disjoint_refs.extend(bearer_refs);
    validate_manifest(&manifest, &disjoint_refs)?;
    Ok(manifest)
}

fn validate_manifest(
    manifest: &Manifest,
    forbidden_refs: &BTreeSet<String>,
) -> Result<(), StoreError> {
    let mut ids = BTreeSet::new();
    let mut refs = BTreeSet::new();
    if manifest.schema_version != SCHEMA
        || !valid_key_id(&manifest.index_key_id)
        || !valid_key_id(&manifest.current_data_key_id)
        || validate_name(&manifest.index_key_ref).is_err()
        || forbidden_refs.contains(&manifest.index_key_ref)
        || manifest.data_keys.is_empty()
        || manifest.data_keys.len() > MAX_DATA_KEYS
        || !refs.insert(manifest.index_key_ref.as_str())
    {
        return Err(StoreError::KeyUnavailable);
    }
    let mut current_present = false;
    for entry in &manifest.data_keys {
        if !valid_key_id(&entry.key_id)
            || validate_name(&entry.file_ref).is_err()
            || forbidden_refs.contains(&entry.file_ref)
            || !ids.insert(entry.key_id.as_str())
            || !refs.insert(entry.file_ref.as_str())
        {
            return Err(StoreError::KeyUnavailable);
        }
        current_present |= entry.key_id == manifest.current_data_key_id;
    }
    if !current_present {
        return Err(StoreError::KeyUnavailable);
    }
    Ok(())
}

fn read_key(root: &PrivateRoot, reference: &str) -> Result<Zeroizing<[u8; 32]>, StoreError> {
    let bytes = root
        .read_file(reference, 32)
        .map_err(|_| StoreError::KeyUnavailable)?;
    let key = bytes
        .as_slice()
        .try_into()
        .map_err(|_| StoreError::KeyUnavailable)?;
    Ok(Zeroizing::new(key))
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}
