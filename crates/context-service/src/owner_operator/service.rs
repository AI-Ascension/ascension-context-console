use crate::authenticated_ingress::{AuthenticatedIngress, AuthenticatedIngressConfig};
use crate::harness_facade::SecretDigest;
use crate::harness_owner_transport::{HarnessOwnerTransport, HarnessOwnerTransportConfig};
use crate::owner_invocation_store::{OwnerInvocationStore, StoreError};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::config::OperatorConfig;
use super::grants::GuardedGrantStore;
use super::principal::FilePrincipalVerifier;
use super::process_lock::{OperatorProcessLock, acquire, cli_error};
use super::protected_files::AdapterError;
use super::provision;
use super::slots::{FileHarnessBearerRedeemer, FileOwnerSlotResolver};

const MAX_REQUEST_LIFETIME: Duration = Duration::from_millis(5_000);

pub(super) struct OperatorService {
    pub(super) config: OperatorConfig,
    pub(super) _process_lock: OperatorProcessLock,
    pub(super) ingress: AuthenticatedIngress<FilePrincipalVerifier, GuardedGrantStore>,
    pub(super) resolver: FileOwnerSlotResolver,
    pub(super) redeemer: FileHarnessBearerRedeemer,
    pub(super) journal: OwnerInvocationStore<super::keys::FileJournalKeyProvider>,
    pub(super) journal_name: String,
    pub(super) journal_identity: (u64, u64),
    pub(super) transport: HarnessOwnerTransport,
}

pub(super) fn run_cli(arguments: Vec<String>) -> Result<(), String> {
    if !cfg!(unix) {
        return Err("owner operator is unsupported on this platform".to_owned());
    }
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage().to_owned());
    };
    match command {
        "serve" => {
            let config_path = config_argument(&arguments[1..])?;
            let config = OperatorConfig::load(&config_path).map_err(map_config_error)?;
            let process_lock =
                acquire(&config.root).map_err(|error| cli_error(error).to_owned())?;
            let mut service = OperatorService::open(config, process_lock).map_err(str::to_owned)?;
            serve(&mut service).map_err(str::to_owned)
        }
        "grant-provision" => {
            let (config_path, input) = config_input_arguments(&arguments[1..])?;
            let config = OperatorConfig::load(&config_path).map_err(map_config_error)?;
            let process_lock =
                acquire(&config.root).map_err(|error| cli_error(error).to_owned())?;
            process_lock
                .check_current()
                .map_err(|error| cli_error(error).to_owned())?;
            provision::provision(&config, &input).map_err(str::to_owned)?;
            process_lock
                .check_current()
                .map_err(|error| cli_error(error).to_owned())?;
            println!("{{\"status\":\"grant_provisioned\"}}");
            Ok(())
        }
        "grant-revoke" => {
            let (config_path, input) = config_input_arguments(&arguments[1..])?;
            let config = OperatorConfig::load(&config_path).map_err(map_config_error)?;
            let process_lock =
                acquire(&config.root).map_err(|error| cli_error(error).to_owned())?;
            process_lock
                .check_current()
                .map_err(|error| cli_error(error).to_owned())?;
            provision::revoke(&config, &input).map_err(str::to_owned)?;
            process_lock
                .check_current()
                .map_err(|error| cli_error(error).to_owned())?;
            println!("{{\"status\":\"grant_revoked\"}}");
            Ok(())
        }
        "journal-key-rotate" => {
            let config_path = config_argument(&arguments[1..])?;
            let config = OperatorConfig::load(config_path).map_err(map_config_error)?;
            let process_lock =
                acquire(&config.root).map_err(|error| cli_error(error).to_owned())?;
            process_lock
                .check_current()
                .map_err(|error| cli_error(error).to_owned())?;
            rotate_journal_keys(&config)?;
            process_lock
                .check_current()
                .map_err(|error| cli_error(error).to_owned())?;
            println!("{{\"status\":\"journal_keys_rotated\"}}");
            Ok(())
        }
        _ => Err(usage().to_owned()),
    }
}

impl OperatorService {
    fn open(
        config: OperatorConfig,
        process_lock: OperatorProcessLock,
    ) -> Result<Self, &'static str> {
        process_lock
            .check_current()
            .map_err(|_| "owner service lock is unavailable")?;
        config
            .root
            .verify_current_root()
            .map_err(|_| "operator state is unavailable")?;
        let verifier = FilePrincipalVerifier::new(&config);
        verifier
            .validate_startup()
            .map_err(|_| "operator principal credentials are unavailable")?;
        super::slots_startup::validate(&config)
            .map_err(|_| "operator Harness credentials are unavailable")?;
        let csrf = config
            .root
            .read_file(&config.csrf_secret_ref, 4 * 1024)
            .map_err(|_| "operator CSRF secret is unavailable")?;
        SecretDigest::from_secret(&csrf).map_err(|_| "operator CSRF secret is invalid")?;

        let grants =
            GuardedGrantStore::open(&config).map_err(|_| "operator grant store is unavailable")?;
        config
            .root
            .verify_current_root()
            .map_err(|_| "operator state is unavailable")?;
        let journal_name = config
            .journal_database
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or("operator journal path is invalid")?
            .to_owned();
        let journal_identity = config
            .root
            .ensure_database_file(&journal_name)
            .map_err(|_| "operator journal store is unavailable")?;
        let journal_path = config
            .root
            .database_path(&journal_name)
            .map_err(|_| "operator journal store is unavailable")?;
        let key_provider = super::keys::FileJournalKeyProvider::new(&config);
        let journal =
            OwnerInvocationStore::open(journal_path, key_provider).map_err(map_journal_error)?;
        config
            .root
            .verify_database_files(&journal_name, Some(journal_identity))
            .map_err(|_| "operator journal store is unavailable")?;
        let transport_config = HarnessOwnerTransportConfig::new(
            config.harness_address,
            Duration::from_millis(config.request_deadline_ms),
        )
        .map_err(|_| "Harness transport configuration is invalid")?;
        let ingress_config =
            AuthenticatedIngressConfig::new(config.issuer.clone(), config.audience.clone())
                .map_err(|_| "operator ingress configuration is invalid")?;
        let ingress = AuthenticatedIngress::new(ingress_config, verifier, grants);
        let resolver = FileOwnerSlotResolver::new(&config);
        let redeemer = FileHarnessBearerRedeemer::new(&config);
        process_lock
            .check_current()
            .map_err(|_| "owner service lock is unavailable")?;
        Ok(Self {
            config,
            _process_lock: process_lock,
            ingress,
            resolver,
            redeemer,
            journal,
            journal_name,
            journal_identity,
            transport: HarnessOwnerTransport::new(transport_config),
        })
    }
}

fn serve(service: &mut OperatorService) -> Result<(), &'static str> {
    service
        ._process_lock
        .check_current()
        .map_err(|_| "owner service lock is unavailable")?;
    service
        .config
        .root
        .verify_current_root()
        .map_err(|_| "operator state is unavailable")?;
    let listener = TcpListener::bind(service.config.listen_address)
        .map_err(|_| "owner listener could not bind")?;
    println!("{{\"status\":\"ready\"}}");
    loop {
        let Ok((mut stream, _peer)) = listener.accept() else {
            continue;
        };
        let deadline = Instant::now()
            + Duration::from_millis(service.config.request_deadline_ms).min(MAX_REQUEST_LIFETIME);
        match super::http::read_request(&mut stream, deadline) {
            Ok(request) => {
                super::routes::serve_request(service, &request, &mut stream, deadline);
            }
            Err(error) => {
                let (status, body) = super::routes::read_error(error);
                let _ = super::http::write_response(&mut stream, status, &body, deadline);
            }
        }
    }
}

fn rotate_journal_keys(config: &OperatorConfig) -> Result<(), String> {
    let name = config
        .journal_database
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| "operator journal path is invalid".to_owned())?
        .to_owned();
    let identity = config
        .root
        .ensure_database_file(&name)
        .map_err(map_config_error)?;
    let path = config.root.database_path(&name).map_err(map_config_error)?;
    let provider = super::keys::FileJournalKeyProvider::new(config);
    let mut store = OwnerInvocationStore::open(path, provider).map_err(map_journal_error)?;
    config
        .root
        .verify_database_files(&name, Some(identity))
        .map_err(map_config_error)?;
    store.rotate_data_keys().map_err(map_journal_error)?;
    config
        .root
        .verify_database_files(&name, Some(identity))
        .map(|_| ())
        .map_err(|error| map_config_error(error).to_owned())
}

fn config_argument(arguments: &[String]) -> Result<PathBuf, String> {
    if arguments.len() != 2 || arguments[0] != "--config" {
        return Err(usage().to_owned());
    }
    let path = PathBuf::from(&arguments[1]);
    if !path.is_absolute() {
        return Err("configuration path must be absolute".to_owned());
    }
    Ok(path)
}

fn config_input_arguments(arguments: &[String]) -> Result<(PathBuf, PathBuf), String> {
    if arguments.len() != 4 || arguments[0] != "--config" || arguments[2] != "--input" {
        return Err(usage().to_owned());
    }
    let config = PathBuf::from(&arguments[1]);
    let input = PathBuf::from(&arguments[3]);
    if !config.is_absolute() || !input.is_absolute() {
        return Err("configuration and input paths must be absolute".to_owned());
    }
    Ok((config, input))
}

fn map_config_error(error: AdapterError) -> &'static str {
    match error {
        AdapterError::Invalid | AdapterError::Denied => "operator configuration is invalid",
        AdapterError::UnsupportedPlatform => "owner operator is unsupported on this platform",
        AdapterError::Unavailable | AdapterError::KeyUnavailable => {
            "operator protected files are unavailable"
        }
    }
}

fn map_journal_error(error: StoreError) -> &'static str {
    match error {
        StoreError::Invalid | StoreError::Denied => "operator journal configuration is invalid",
        StoreError::UnsupportedPlatform => "owner operator is unsupported on this platform",
        StoreError::KeyUnavailable | StoreError::CryptoUnavailable => {
            "operator journal keys are unavailable"
        }
        StoreError::Capacity | StoreError::StorageLimit => "operator journal limit reached",
        _ => "operator journal is unavailable",
    }
}

fn usage() -> &'static str {
    "owner command requires serve|grant-provision|grant-revoke|journal-key-rotate and an absolute config path"
}
