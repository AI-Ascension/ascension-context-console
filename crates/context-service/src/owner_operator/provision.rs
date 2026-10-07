use crate::control::Scope;
use crate::harness_facade::FacadePermission;
use crate::subject_grants::{SubjectGrantError, SubjectGrantSpec};
use serde::Deserialize;
use std::path::Path;

use super::config::OperatorConfig;
use super::grants::GuardedGrantStore;
use super::protected_files::{AdapterError, read_private_file};

const MAX_INPUT_BYTES: usize = 16 * 1024;
const PROVISION_SCHEMA: &str = "ascension.context-console.subject-grant-provision.v1";
const REVOKE_SCHEMA: &str = "ascension.context-console.subject-grant-revoke.v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionInput {
    schema_version: String,
    grant_id: String,
    issuer: String,
    subject: String,
    permission: String,
    scope: Scope,
    not_before: u64,
    expires_at: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevokeInput {
    schema_version: String,
    grant_id: String,
}

pub(super) fn provision(config: &OperatorConfig, input: &Path) -> Result<(), &'static str> {
    config
        .root
        .verify_current_root()
        .map_err(|_| "operator state is unavailable")?;
    let bytes = read_private_file(input, MAX_INPUT_BYTES).map_err(map_file_error)?;
    let raw: ProvisionInput =
        serde_json::from_slice(&bytes).map_err(|_| "grant input is invalid")?;
    let permission = parse_permission(&raw.permission).ok_or("grant input is invalid")?;
    if raw.schema_version != PROVISION_SCHEMA
        || raw.issuer != config.issuer
        || raw.scope != config.scope
        || !crate::harness_facade::valid_id(&raw.grant_id)
        || !crate::harness_facade::valid_id(&raw.subject)
        || raw.not_before >= raw.expires_at
    {
        return Err("grant input is invalid");
    }
    let spec = SubjectGrantSpec {
        grant_id: raw.grant_id,
        issuer: raw.issuer,
        subject: raw.subject,
        permission,
        scope: raw.scope,
        not_before: raw.not_before,
        expires_at: raw.expires_at,
    };
    let mut store = GuardedGrantStore::open(config).map_err(map_store_error)?;
    store.provision(&spec).map_err(map_store_error)?;
    config
        .root
        .verify_current_root()
        .map_err(|_| "operator state is unavailable")
}

pub(super) fn revoke(config: &OperatorConfig, input: &Path) -> Result<(), &'static str> {
    config
        .root
        .verify_current_root()
        .map_err(|_| "operator state is unavailable")?;
    let bytes = read_private_file(input, MAX_INPUT_BYTES).map_err(map_file_error)?;
    let raw: RevokeInput = serde_json::from_slice(&bytes).map_err(|_| "grant input is invalid")?;
    if raw.schema_version != REVOKE_SCHEMA || !crate::harness_facade::valid_id(&raw.grant_id) {
        return Err("grant input is invalid");
    }
    let mut store = GuardedGrantStore::open(config).map_err(map_store_error)?;
    store.revoke(&raw.grant_id).map_err(map_store_error)?;
    config
        .root
        .verify_current_root()
        .map_err(|_| "operator state is unavailable")
}

fn parse_permission(value: &str) -> Option<FacadePermission> {
    Some(match value {
        "context.metadata.read" => FacadePermission::MetadataRead,
        "context.content.read" => FacadePermission::ContentRead,
        "context.content.write" => FacadePermission::ContentWrite,
        "context.edit" => FacadePermission::Edit,
        "context.objective.edit" => FacadePermission::Objective,
        "context.commit" => FacadePermission::Commit,
        "context.pause" => FacadePermission::Pause,
        "context.resume" => FacadePermission::Resume,
        _ => return None,
    })
}

fn map_file_error(error: AdapterError) -> &'static str {
    match error {
        AdapterError::Invalid | AdapterError::Denied => "grant input is invalid",
        AdapterError::UnsupportedPlatform
        | AdapterError::Unavailable
        | AdapterError::KeyUnavailable => "operator state is unavailable",
    }
}

fn map_store_error(error: SubjectGrantError) -> &'static str {
    match error {
        SubjectGrantError::Invalid => "grant input is invalid",
        SubjectGrantError::Duplicate => "grant identity already exists",
        SubjectGrantError::NotFound => "grant is unavailable",
        SubjectGrantError::Capacity | SubjectGrantError::StorageLimit => {
            "operator grant store limit reached"
        }
        SubjectGrantError::Denied
        | SubjectGrantError::Corrupt
        | SubjectGrantError::StoreUnavailable => "operator grant store is unavailable",
    }
}
