use std::env;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use sts2_harness::management::{RunRequest, digest_value, live_run_id};
use sts2_harness::provider_session::{
    ContinuityMode, NativeCapabilities, ProviderSessionMetadataStore, ProviderSessionMode,
    ProviderSessionPolicy, ProviderSessionPolicyOwner, SessionScope,
};

const BASELINE_POLICY_ID: &str = "console-production-baseline";
const MIGRATION_POLICY_ID: &str = "console-production-migration";
const PROJECT_ID: &str = "served-policy-project";
const REQUEST_ID: &str = "served-policy-gate";
const EPISODE_ID: &str = "episode-served-policy-gate";
const AGENT_ID: &str = "served-policy-agent";
const INSTANCE_ID: &str = "instance-1";
const POLICY_KEY_REFERENCE: &str = "STS2_SERVED_PROVIDER_POLICY_KEY";

fn main() {
    if let Err(error) = run() {
        eprintln!("provider-policy fixture failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let operation = args.next().ok_or("expected bootstrap or verify")?;
    let store_path = PathBuf::from(args.next().ok_or("expected owner store path")?);
    let definition_path = PathBuf::from(args.next().ok_or("expected definition path")?);
    let key = provider_key()?;
    let definition = served_definition(&definition_path)?;
    let definition_digest = digest_value(&definition)?;
    let request = RunRequest {
        schema_version: sts2_harness::management::MANAGEMENT_SCHEMA_VERSION.to_owned(),
        request_id: REQUEST_ID.to_owned(),
        definition: Some(definition),
        artifact_id: None,
        instance_id: INSTANCE_ID.to_owned(),
        profile: "live.workflow.v1".to_owned(),
        admission: None,
    };
    let run_id = live_run_id(&request, &definition_digest)?;
    let scope = policy_scope(&run_id)?;
    let mut capabilities = NativeCapabilities::fixture();
    capabilities.effective_limits.max_completed_turns = 1;
    capabilities.binding.descriptor_sha256 = capabilities.descriptor_digest();
    capabilities.validate()?;

    match operation.as_str() {
        "bootstrap" => {
            let source_path = PathBuf::from(args.next().ok_or("expected migration source path")?);
            let target_path = PathBuf::from(args.next().ok_or("expected migration target path")?);
            let store = ProviderSessionMetadataStore::encrypted(&store_path, key, scope.clone())?;
            let owner =
                ProviderSessionPolicyOwner::open(store, scope.clone(), capabilities.clone())?;
            let baseline = baseline_policy(scope.clone(), &capabilities);
            let baseline_bytes = serde_json::to_vec(&baseline)?;
            let baseline_sha = owner.import(baseline_bytes)?;
            owner.adopt_imported(&baseline_sha, 2)?;
            let source = migration_source(scope.clone(), &capabilities);
            let target = migration_target(source.clone());
            write_private(&source_path, &serde_json::to_vec_pretty(&source)?)?;
            write_private(&target_path, &serde_json::to_vec_pretty(&target)?)?;
            let metadata = owner.metadata()?;
            if metadata.revision != 3
                || metadata
                    .active
                    .as_ref()
                    .map(|policy| policy.policy_id.as_str())
                    != Some(BASELINE_POLICY_ID)
            {
                return Err(
                    "baseline owner state did not reach its expected adopted revision".into(),
                );
            }
            let config = provider_policy_config(&store_path, scope.clone(), &capabilities);
            let fixture = json!({
                "run_id": run_id,
                "definition_digest": definition_digest,
                "request_id": REQUEST_ID,
                "instance_id": INSTANCE_ID,
                "provider_policy_config": config,
                "baseline_revision": metadata.revision,
                "source_policy_path": source_path,
                "target_policy_path": target_path,
            });
            println!("{}", serde_json::to_string(&fixture)?);
        }
        "verify" => {
            let source_path =
                PathBuf::from(args.next().ok_or("expected migration source policy path")?);
            let target_path =
                PathBuf::from(args.next().ok_or("expected migration target policy path")?);
            if args.next().is_some() {
                return Err("verify accepts only migration source and target paths".into());
            }
            let store = ProviderSessionMetadataStore::encrypted(&store_path, key, scope.clone())?;
            let owner = ProviderSessionPolicyOwner::open(store, scope, capabilities)?;
            let metadata = owner.metadata()?;
            let source_bytes = fs::read(source_path)?;
            let target_bytes = fs::read(target_path)?;
            let source_sha = sts2_harness::sha256_hex(&source_bytes);
            let target_sha = sts2_harness::sha256_hex(&target_bytes);
            let proposal = metadata
                .proposals
                .iter()
                .find(|entry| entry.proposal_id == "migration.console.production")
                .ok_or("durable owner journal is missing the browser-created proposal")?;
            let active = metadata
                .active
                .as_ref()
                .ok_or("durable owner journal has no active policy")?;
            if metadata.revision != 7
                || metadata.policies.len() != 3
                || metadata.proposals.len() != 1
                || metadata.scope.run_id != run_id
                || !metadata
                    .policies
                    .iter()
                    .any(|policy| policy.sha256 == source_sha)
                || active.sha256 != target_sha
                || active.policy_id != MIGRATION_POLICY_ID
                || active.version != 2
                || proposal.state
                    != sts2_harness::provider_session::SessionPolicyMigrationState::Adopted
                || proposal.adopted_policy_sha256.as_deref() != Some(target_sha.as_str())
            {
                return Err(
                    "reopened encrypted journal does not contain the expected browser adoption"
                        .into(),
                );
            }
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "reopened": true,
                    "run_id": run_id,
                    "revision": metadata.revision,
                    "history_count": metadata.policies.len(),
                    "proposal_state": "adopted",
                    "active_policy_id": active.policy_id,
                    "source_policy_sha256": source_sha,
                    "target_policy_sha256": target_sha,
                    "effect_class": "local_metadata_only",
                    "inference_calls": 0,
                    "game_effects": 0,
                }))?
            );
        }
        _ => return Err("expected bootstrap or verify".into()),
    }
    Ok(())
}

fn provider_key() -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let encoded = env::var(POLICY_KEY_REFERENCE)?;
    if encoded.len() != 64 {
        return Err("provider policy fixture key must be 64 hexadecimal characters".into());
    }
    let mut key = [0_u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&encoded[index * 2..index * 2 + 2], 16)?;
    }
    if key.iter().all(|byte| *byte == 0) {
        return Err("provider policy fixture key must not be zero".into());
    }
    Ok(key)
}

fn served_definition(path: &PathBuf) -> Result<Value, Box<dyn std::error::Error>> {
    let mut definition: Value = serde_json::from_slice(&fs::read(path)?)?;
    definition["annotations"]["synthetic"] = json!(false);
    definition["game_profile"] = json!("sts2-live-v1");
    definition["policy_ref"] = json!("policy.live.v1");
    definition["graphs"][0]["nodes"][0]["config"]["projection_ref"] = json!("fair-play.live.v1");
    definition["graphs"][0]["nodes"][1]["config"]["decision_profile_ref"] =
        json!("decision.live.v1");
    definition["graphs"][0]["nodes"][1]["config"]["context_ref"] = json!("context.live.v1");
    definition["capabilities"]["required"][0] = json!("observe.fair-play.v1");
    Ok(definition)
}

fn policy_scope(run_id: &str) -> Result<SessionScope, Box<dyn std::error::Error>> {
    Ok(SessionScope::new(PROJECT_ID, run_id, EPISODE_ID, AGENT_ID)?)
}

fn baseline_policy(
    scope: SessionScope,
    capabilities: &NativeCapabilities,
) -> ProviderSessionPolicy {
    let mut policy = ProviderSessionPolicy::disabled(scope);
    policy.policy_id = BASELINE_POLICY_ID.to_owned();
    policy.mode = ProviderSessionMode::FixtureOnly;
    policy.continuity = ContinuityMode::StrictReviewed;
    policy.credential_realm_ref = "console-fixture-realm".to_owned();
    policy.profile_sha256 = capabilities.profile_sha256.clone();
    policy.max_completed_turns = 1;
    policy
}

fn migration_source(
    scope: SessionScope,
    capabilities: &NativeCapabilities,
) -> ProviderSessionPolicy {
    let mut policy = ProviderSessionPolicy::disabled(scope);
    policy.policy_id = MIGRATION_POLICY_ID.to_owned();
    policy.mode = ProviderSessionMode::FixtureOnly;
    policy.continuity = ContinuityMode::StrictReviewed;
    policy.credential_realm_ref = "console-fixture-realm".to_owned();
    policy.profile_sha256 = capabilities.profile_sha256.clone();
    policy.max_completed_turns = 1_024;
    policy.version = 1;
    policy.epoch = 1;
    policy
}

fn migration_target(mut source: ProviderSessionPolicy) -> ProviderSessionPolicy {
    source.version = 2;
    source.max_completed_turns = 1;
    source.epoch = 2;
    source
}

fn provider_policy_config(
    store_path: &PathBuf,
    scope: SessionScope,
    capabilities: &NativeCapabilities,
) -> Value {
    json!({
        "schema_version": "ascension.workflow-provider-policy-config.v1",
        "store_path": store_path,
        "key_reference": POLICY_KEY_REFERENCE,
        "scope": scope,
        "capabilities": capabilities,
        "selected_profile": capabilities.profile_id,
    })
}

fn write_private(path: &PathBuf, bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
