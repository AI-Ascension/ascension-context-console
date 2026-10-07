use crate::authenticated_ingress::AuthenticatedIngressConfig;
use crate::control::Scope;
use crate::harness_facade::{HarnessFacadeConfig, RetentionPolicy, valid_id};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use super::protected_files::{AdapterError, PrivateRoot, read_private_file, validate_name};

pub(super) const CONFIG_SCHEMA: &str = "ascension.context-console.owner-service.v1";
const MAX_CONFIG_BYTES: usize = 16 * 1024;
const RESERVED_LOCK_NAME: &str = "owner-service.lock";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    schema_version: String,
    protected_source: String,
    listen_address: String,
    harness_address: String,
    owner_id: String,
    scope: Scope,
    issuer: String,
    audience: String,
    expected_host: String,
    expected_origin: Option<String>,
    csrf_secret_ref: String,
    state_root: PathBuf,
    principal_registry_ref: String,
    principal_mac_key_ref: String,
    owner_slots_ref: String,
    journal_key_manifest_ref: String,
    grant_database: String,
    journal_database: String,
    request_deadline_ms: u64,
}

pub(super) struct OperatorConfig {
    pub(super) listen_address: SocketAddr,
    pub(super) harness_address: SocketAddr,
    pub(super) owner_id: String,
    pub(super) scope: Scope,
    pub(super) issuer: String,
    pub(super) audience: String,
    pub(super) expected_host: String,
    pub(super) expected_origin: Option<String>,
    pub(super) csrf_secret_ref: String,
    pub(super) principal_registry_ref: String,
    pub(super) principal_mac_key_ref: String,
    pub(super) owner_slots_ref: String,
    pub(super) journal_key_manifest_ref: String,
    pub(super) grant_database: PathBuf,
    pub(super) journal_database: PathBuf,
    pub(super) request_deadline_ms: u64,
    pub(super) root: PrivateRoot,
}

impl OperatorConfig {
    pub(super) fn load(path: impl AsRef<Path>) -> Result<Self, AdapterError> {
        let bytes = read_private_file(path.as_ref(), MAX_CONFIG_BYTES)?;
        let raw: RawConfig = serde_json::from_slice(&bytes).map_err(|_| AdapterError::Invalid)?;
        if raw.schema_version != CONFIG_SCHEMA
            || raw.protected_source != "protected-private-files-v1"
            || !valid_id(&raw.owner_id)
            || !(1..=5_000).contains(&raw.request_deadline_ms)
            || !raw.state_root.is_absolute()
        {
            return Err(AdapterError::Invalid);
        }
        let listen_address = raw
            .listen_address
            .parse::<SocketAddr>()
            .map_err(|_| AdapterError::Invalid)?;
        let harness_address = raw
            .harness_address
            .parse::<SocketAddr>()
            .map_err(|_| AdapterError::Invalid)?;
        if !listen_address.ip().is_loopback()
            || listen_address.port() == 0
            || !harness_address.ip().is_loopback()
            || harness_address.port() == 0
        {
            return Err(AdapterError::Invalid);
        }
        let scope = Scope::new(
            raw.scope.project_id,
            raw.scope.run_id,
            raw.scope.episode_id,
            raw.scope.agent_id,
        )
        .map_err(|_| AdapterError::Invalid)?;
        AuthenticatedIngressConfig::new(raw.issuer.clone(), raw.audience.clone())
            .map_err(|_| AdapterError::Invalid)?;
        HarnessFacadeConfig::new(
            scope.clone(),
            raw.expected_host.clone(),
            raw.expected_origin.clone(),
            None,
            RetentionPolicy::default(),
        )
        .map_err(|_| AdapterError::Invalid)?;
        let references = [
            raw.csrf_secret_ref.as_str(),
            raw.principal_registry_ref.as_str(),
            raw.principal_mac_key_ref.as_str(),
            raw.owner_slots_ref.as_str(),
            raw.journal_key_manifest_ref.as_str(),
            RESERVED_LOCK_NAME,
        ];
        for reference in references {
            validate_name(reference)?;
        }
        if has_duplicates(&references) {
            return Err(AdapterError::Invalid);
        }
        for name in [&raw.grant_database, &raw.journal_database] {
            validate_name(name)?;
        }
        let mut database_artifacts = BTreeSet::new();
        for database in [&raw.grant_database, &raw.journal_database] {
            for artifact in database_artifacts_for_name(Path::new(database)) {
                if !database_artifacts.insert(artifact) {
                    return Err(AdapterError::Invalid);
                }
            }
        }
        if database_artifacts.contains(RESERVED_LOCK_NAME) {
            return Err(AdapterError::Invalid);
        }
        if references
            .iter()
            .any(|reference| database_artifacts.contains(*reference))
        {
            return Err(AdapterError::Invalid);
        }
        let root = PrivateRoot::open(&raw.state_root)?;
        Ok(Self {
            listen_address,
            harness_address,
            owner_id: raw.owner_id,
            scope,
            issuer: raw.issuer,
            audience: raw.audience,
            expected_host: raw.expected_host,
            expected_origin: raw.expected_origin,
            csrf_secret_ref: raw.csrf_secret_ref,
            principal_registry_ref: raw.principal_registry_ref,
            principal_mac_key_ref: raw.principal_mac_key_ref,
            owner_slots_ref: raw.owner_slots_ref,
            journal_key_manifest_ref: raw.journal_key_manifest_ref,
            grant_database: raw.state_root.join(raw.grant_database),
            journal_database: raw.state_root.join(raw.journal_database),
            request_deadline_ms: raw.request_deadline_ms,
            root,
        })
    }

    pub(super) fn protected_file_refs(&self) -> BTreeSet<String> {
        let mut refs = [
            self.csrf_secret_ref.clone(),
            self.principal_registry_ref.clone(),
            self.principal_mac_key_ref.clone(),
            self.owner_slots_ref.clone(),
            self.journal_key_manifest_ref.clone(),
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        refs.insert(RESERVED_LOCK_NAME.to_owned());
        refs.extend(database_artifacts_for_name(&self.grant_database));
        refs.extend(database_artifacts_for_name(&self.journal_database));
        refs
    }
}

fn has_duplicates(values: &[&str]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

fn database_artifacts_for_name(path: &Path) -> Vec<String> {
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
        return Vec::new();
    };
    std::iter::once(name.to_owned())
        .chain(
            ["-wal", "-shm", "-journal"]
                .into_iter()
                .map(|suffix| format!("{name}{suffix}")),
        )
        .collect()
}
