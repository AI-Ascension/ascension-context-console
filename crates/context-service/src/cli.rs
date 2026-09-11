// SPDX-License-Identifier: MIT

//! Bounded operator CLI for the synthetic Phase 2 control store.
//!
//! This is intentionally a fixture command surface. It uses the same typed reducer and strict
//! payload parser as the integrated HTTP path, keeps private note text on stdin, and emits metadata
//! by default. A production deployment must replace the fixture key and capability plumbing.

use crate::provider_session::ProviderSessionRoute;
use crate::{
    ControlCommand, ControlError, ControlMemoryBindingRecord, ControlOperation, ControlPatch,
    ControlPlane, DurableControlStore, MAX_MEMORY_BODY_BYTES, MemoryQueryRequest, MemoryRoute,
    MemoryScope, parse_control_json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const CLI_SCHEMA: &str = "ascension.context-control.cli-result.v1";
const RUN_ID: &str = "fixture-run";
const STORE_KEY: [u8; 32] = [0x42; 32];
const OBJECTIVE_TOKEN: &str = "fixture-objective-token";
const CONTENT_TOKEN: &str = "fixture-content-token";
const MAX_PATCH_BYTES: usize = 16 * 1024;

const PHASE3_CLI_SCHEMA: &str = "ascension.context-memory.cli-result.v1";
const PHASE3_ADAPTER_SCHEMA: &str = "ascension.context-memory.adapter-result.v1";
const PHASE3_ADAPTER_REQUEST_SCHEMA: &str = "ascension.context-memory.adapter-request.v1";

const PHASE4_CLI_SCHEMA: &str = "ascension.provider-session.cli-result.v1";

pub fn run_phase2_cli(arguments: Vec<String>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let Some(operation) = arguments.next() else {
        print_help();
        return Ok(());
    };
    if matches!(operation.as_str(), "help" | "--help" | "-h") {
        print_help();
        return Ok(());
    }
    if operation == "init" {
        let path = required_path(&mut arguments)?;
        ensure_no_arguments(&mut arguments, "init")?;
        if path.exists() {
            return cli_error(
                "init",
                "store_exists",
                "refusing to overwrite an existing store",
            );
        }
        let plane = ControlPlane::synthetic();
        DurableControlStore::create(&path, STORE_KEY, &plane).map_err(store_error)?;
        return emit("init", "completed", json!({"path": path, "run_id": RUN_ID}));
    }

    let path = required_path(&mut arguments)?;
    let (mut store, plane) = open_store(&path)?;
    match operation.as_str() {
        "state" => {
            ensure_no_arguments(&mut arguments, "state")?;
            emit(
                "state",
                "completed",
                serde_json::to_value(plane.state()).map_err(encode)?,
            )
        }
        "capabilities" => {
            ensure_no_arguments(&mut arguments, "capabilities")?;
            emit("capabilities", "completed", capabilities_value(&plane))
        }
        "eligible" => {
            let include_content = arguments.next().as_deref() == Some("--include-content");
            ensure_no_arguments(&mut arguments, "eligible")?;
            if include_content
                && env::var("CONTEXT_CONSOLE_CONTENT_TOKEN").ok().as_deref() != Some(CONTENT_TOKEN)
            {
                return cli_error(
                    "eligible",
                    "content_permission_required",
                    "explicit content permission is required",
                );
            }
            let mut items = serde_json::to_value(plane.eligible_items()).map_err(encode)?;
            if !include_content && let Some(entries) = items.as_array_mut() {
                for entry in entries {
                    if let Some(object) = entry.as_object_mut() {
                        object.insert("content".to_owned(), Value::Null);
                    }
                }
            }
            emit("eligible", "completed", json!({"items": items}))
        }
        "revisions" => {
            ensure_no_arguments(&mut arguments, "revisions")?;
            emit(
                "revisions",
                "completed",
                json!({"revisions": plane.revisions()}),
            )
        }
        "drafts" => {
            ensure_no_arguments(&mut arguments, "drafts")?;
            emit("drafts", "completed", json!({"drafts": plane.drafts()}))
        }
        "events" => {
            ensure_no_arguments(&mut arguments, "events")?;
            emit("events", "completed", json!({"events": plane.events()}))
        }
        "draft-show" => {
            let draft_id = required_argument(&mut arguments, "draft_id")?;
            ensure_no_arguments(&mut arguments, "draft-show")?;
            emit(
                "draft-show",
                "completed",
                serde_json::to_value(plane.get_draft(&draft_id).map_err(control_error)?)
                    .map_err(encode)?,
            )
        }
        "preview-show" => {
            let preview_id = required_argument(&mut arguments, "preview_id")?;
            ensure_no_arguments(&mut arguments, "preview-show")?;
            emit(
                "preview-show",
                "completed",
                serde_json::to_value(plane.get_preview(&preview_id).map_err(control_error)?)
                    .map_err(encode)?,
            )
        }
        "command" => {
            let command_id = required_argument(&mut arguments, "command_id")?;
            ensure_no_arguments(&mut arguments, "command")?;
            let value = serde_json::to_value(plane.command(&command_id).map_err(control_error)?)
                .map_err(encode)?;
            emit("command", response_status(&value), value)
        }
        "draft-create" => {
            ensure_no_arguments(&mut arguments, "draft-create")?;
            let scope = plane.scope().clone();
            let active = plane.state().active_revision_id.clone();
            mutate(&mut store, plane, "draft-create", |candidate| {
                candidate.create_draft(scope, &active, "operator-cli")
            })
        }
        "draft-edit" => {
            let author = arguments
                .next()
                .unwrap_or_else(|| "operator-cli".to_owned());
            ensure_no_arguments(&mut arguments, "draft-edit")?;
            let body = stdin_bytes()?;
            let patch: ControlPatch = parse_control_json(&body).map_err(control_error)?;
            let objective = env::var("CONTEXT_CONSOLE_OBJECTIVE_TOKEN").ok().as_deref()
                == Some(OBJECTIVE_TOKEN);
            mutate(&mut store, plane, "draft-edit", |candidate| {
                candidate.apply_patch(patch, &author, objective)
            })
        }
        "preview" => {
            let draft_id = required_argument(&mut arguments, "draft_id")?;
            let version = parse_u64(&mut arguments, "draft_version")?;
            let applicable = parse_bool(&mut arguments, "applicable")?;
            let risk_ack = parse_optional_bool(&mut arguments, "risk_ack")?;
            ensure_no_arguments(&mut arguments, "preview")?;
            let scope = plane.scope().clone();
            let control_version = plane.state().control_version;
            mutate(&mut store, plane, "preview", |candidate| {
                candidate.create_preview(
                    scope,
                    draft_id.as_str(),
                    version,
                    applicable,
                    control_version,
                    risk_ack,
                )
            })
        }
        "pause" => {
            let key = required_argument(&mut arguments, "idempotency_key")?;
            ensure_no_arguments(&mut arguments, "pause")?;
            let command = command_for(&plane, "pause", key, None, None, None);
            mutate(&mut store, plane, "pause", |candidate| {
                candidate.pause(command)
            })
        }
        "commit" => {
            let key = required_argument(&mut arguments, "idempotency_key")?;
            let preview_id = required_argument(&mut arguments, "preview_id")?;
            ensure_no_arguments(&mut arguments, "commit")?;
            let preview = plane.get_preview(&preview_id).map_err(control_error)?;
            let command = command_for(
                &plane,
                "commit",
                key,
                Some(plane.state().active_revision_id.clone()),
                Some(preview_id),
                preview.prepared_manifest_sha256,
            );
            mutate(&mut store, plane, "commit", |candidate| {
                candidate.commit(command)
            })
        }
        "resume" => {
            let key = required_argument(&mut arguments, "idempotency_key")?;
            let preview_id = required_argument(&mut arguments, "preview_id")?;
            ensure_no_arguments(&mut arguments, "resume")?;
            let command = command_for(
                &plane,
                "resume",
                key,
                Some(plane.state().active_revision_id.clone()),
                Some(preview_id),
                None,
            );
            mutate(&mut store, plane, "resume", |candidate| {
                candidate.resume(command)
            })
        }
        _ => cli_error(
            &operation,
            "unsupported_command",
            "phase2-cli command is unavailable",
        ),
    }
}

/// Run the bounded target facade for Phase 3 memory operations.  Corpus mutation remains owned
/// by the harness; this command exposes capability/status/search through the same validated route
/// and deliberately reports a projection-unavailable result until a harness adapter is attached.
pub fn run_phase3_cli(arguments: Vec<String>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let operation = arguments.next().unwrap_or_else(|| "help".to_owned());
    if matches!(operation.as_str(), "help" | "--help" | "-h") {
        println!("phase3-cli capabilities|status|search");
        println!("  search reads a bounded query.v1 document from stdin");
        return Ok(());
    }
    ensure_no_arguments(&mut arguments, "phase3-cli")?;
    let scope = MemoryScope {
        project_id: "project-fixture".to_owned(),
        run_id: "run-fixture".to_owned(),
        episode_id: "episode-fixture".to_owned(),
        agent_id: "agent-fixture".to_owned(),
    };
    // The target CLI has no attached harness projection.  Keep the route discoverable while
    // leaving memory disabled until an explicit adapter supplies corpus generations and bytes.
    let mut route = MemoryRoute::new(scope, false);
    route.grant_search("operator-cli");
    route.grant_review("reviewer-cli");
    let value = match operation.as_str() {
        "capabilities" => route
            .handle("GET", "/v3/memory/capabilities", "operator-cli", &[])
            .map_err(|error| error.to_string())?,
        "status" => route
            .handle("GET", "/v3/memory/status", "operator-cli", &[])
            .map_err(|error| error.to_string())?,
        "search" => {
            let body = stdin_bytes_bounded(MAX_MEMORY_BODY_BYTES)?;
            let _: MemoryQueryRequest = serde_json::from_slice(&body)
                .map_err(|_| "phase3-cli: invalid query JSON".to_owned())?;
            route
                .handle("POST", "/v3/memory/search", "operator-cli", &body)
                .map_err(|error| error.to_string())?
        }
        _ => {
            return cli_error(
                "phase3-cli",
                "unsupported_command",
                "memory command is unavailable",
            );
        }
    };
    let output = json!({
        "schema": PHASE3_CLI_SCHEMA,
        "operation": operation,
        "status": "completed",
        "value": value
    });
    println!("{}", serde_json::to_string(&output).map_err(encode)?);
    Ok(())
}

/// Run the bounded Phase 4 client projection. The target owns no native process or credential;
/// fixture commands return typed metadata/operation identities and keep inference/game effects at
/// zero. Real session mutation remains behind the harness scheduler and is never a CLI passthrough.
pub fn run_phase4_cli(arguments: Vec<String>) -> Result<(), String> {
    let mut arguments = arguments.into_iter();
    let operation = arguments.next().unwrap_or_else(|| "help".to_owned());
    if matches!(operation.as_str(), "help" | "--help" | "-h") {
        println!(
            "phase4-cli capabilities|list|candidate|binding <id>|history <id>|reconnect <id>|fork <id>|compact <id>|retire <id>|cleanup <id>|operation <id>"
        );
        println!("  candidate/reconnect/fork/compact/retire/cleanup accept bounded JSON on stdin");
        return Ok(());
    }
    let argument = arguments.next();
    if arguments.next().is_some() {
        return Err("phase4-cli: unexpected argument".to_owned());
    }
    let mut route = ProviderSessionRoute::fixture("operator-cli");
    let run = "run-fixture";
    let base = format!("/v1/runs/{run}/provider-sessions");
    let (method, path, body) = match operation.as_str() {
        "capabilities" => ("GET", format!("{base}/capabilities"), Vec::new()),
        "list" | "sessions" => ("GET", base.clone(), Vec::new()),
        "candidate" | "create" => (
            "POST",
            format!("{base}/candidates"),
            stdin_bytes_bounded(16 * 1024)?,
        ),
        "binding" => (
            "GET",
            format!(
                "{base}/{}",
                argument.ok_or_else(|| "phase4-cli: binding id is required".to_owned())?
            ),
            Vec::new(),
        ),
        "history" => (
            "GET",
            format!(
                "{base}/{}/history",
                argument.ok_or_else(|| "phase4-cli: binding id is required".to_owned())?
            ),
            Vec::new(),
        ),
        "reconnect" | "fork" | "compact" | "retire" | "cleanup" => {
            let binding =
                argument.ok_or_else(|| "phase4-cli: binding id is required".to_owned())?;
            let suffix = match operation.as_str() {
                "reconnect" => "reconnect",
                "fork" => "fork-jobs",
                "compact" => "compaction-jobs",
                "retire" => "retire",
                _ => "cleanup",
            };
            (
                "POST",
                format!("{base}/{binding}/{suffix}"),
                stdin_bytes_bounded(16 * 1024)?,
            )
        }
        "operation" => {
            let id = argument.ok_or_else(|| "phase4-cli: operation id is required".to_owned())?;
            (
                "GET",
                format!("/v1/runs/{run}/provider-session-operations/{id}"),
                Vec::new(),
            )
        }
        _ => return Err("phase4-cli: unsupported command".to_owned()),
    };
    let value = route
        .handle(method, &path, "operator-cli", &body)
        .map_err(|error| error.to_string())?;
    let status = if method == "POST" {
        "accepted"
    } else {
        "completed"
    };
    println!(
        "{}",
        serde_json::to_string(
            &json!({"schema":PHASE4_CLI_SCHEMA,"operation":operation,"status":status,"value":value}),
        )
        .map_err(encode)?
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Phase3AdapterRequest {
    schema: String,
    binding_id: String,
    selection_id: String,
    phase2_revision_id: String,
    selection_sha256: String,
    audit_sha256: String,
    policy_id: String,
    policy_version: u64,
    review_id: String,
    summary_output_sha256: String,
    first_input_sha256: String,
    preview_only: bool,
}

/// Execute the bounded target side of the cross-repository Phase 3 fixture adapter. The harness
/// sends reviewed selection metadata; this process runs the real Phase 2 reducer and commits the
/// memory binding beside the encrypted revision journal. No provider, game, network or child
/// process path is available here.
pub fn run_phase3_adapter() -> Result<(), String> {
    let body = stdin_bytes_bounded(MAX_MEMORY_BODY_BYTES)?;
    let request: Phase3AdapterRequest = parse_control_json(&body).map_err(control_error)?;
    if request.schema != PHASE3_ADAPTER_REQUEST_SCHEMA
        || !wire_id(&request.binding_id)
        || !wire_id(&request.selection_id)
        || !wire_id(&request.phase2_revision_id)
        || !wire_digest(&request.selection_sha256)
        || !wire_digest(&request.audit_sha256)
        || !wire_id(&request.policy_id)
        || request.policy_version == 0
        || !wire_id(&request.review_id)
        || !wire_digest(&request.summary_output_sha256)
        || !wire_digest(&request.first_input_sha256)
    {
        return Err("phase3-adapter: invalid request".to_owned());
    }

    let mut paused = ControlPlane::synthetic();
    let scope = paused.scope().clone();
    let active_revision = paused.state().active_revision_id.clone();
    let draft = paused
        .create_draft(scope.clone(), &active_revision, "phase3-adapter")
        .map_err(control_error)?;
    let history = paused
        .eligible_items()
        .into_iter()
        .find(|item| item.item.item_id == "history-1")
        .ok_or_else(|| "phase3-adapter: history fixture is unavailable".to_owned())?
        .item;
    let edited = paused
        .apply_patch(
            ControlPatch {
                schema: "ascension.context-control.patch.v1".to_owned(),
                scope: scope.clone(),
                draft_id: draft.draft_id.clone(),
                expected_draft_version: draft.version,
                expected_active_revision_id: active_revision,
                operations: vec![ControlOperation::IncludeItem { item: history }],
            },
            "phase3-adapter",
            false,
        )
        .map_err(control_error)?;
    paused
        .pause(command_for(
            &paused,
            "pause",
            "phase3-adapter-pause".to_owned(),
            None,
            None,
            None,
        ))
        .map_err(control_error)?;
    let preview = paused
        .create_preview(
            scope,
            &edited.draft_id,
            edited.version,
            true,
            paused.state().control_version,
            true,
        )
        .map_err(control_error)?;
    let commit_command = command_for(
        &paused,
        "commit",
        "phase3-adapter-commit".to_owned(),
        Some(paused.state().active_revision_id.clone()),
        Some(preview.preview_id.clone()),
        preview.prepared_manifest_sha256.clone(),
    );
    let mut planned = paused.clone();
    planned
        .commit(commit_command.clone())
        .map_err(control_error)?;
    if request.preview_only {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "schema": PHASE3_ADAPTER_SCHEMA,
                "status": "preview_ready",
                "selection_id": request.selection_id,
                "phase2_preview_id": preview.preview_id,
                "planned_phase2_revision_id": planned.state().active_revision_id,
                "prepared_manifest_sha256": preview.prepared_manifest_sha256,
                "provider_calls": 0,
                "game_launches": 0,
                "external_requests": 0,
            }))
            .map_err(encode)?
        );
        return Ok(());
    }
    if request.phase2_revision_id != planned.state().active_revision_id
        || request.first_input_sha256
            != preview
                .prepared_manifest_sha256
                .as_deref()
                .unwrap_or_default()
    {
        return Err("phase3-adapter: prepared Phase 2 identity changed".to_owned());
    }
    let mut committed = paused.clone();
    let receipt = committed.commit(commit_command).map_err(control_error)?;
    let binding = ControlMemoryBindingRecord {
        schema: "ascension.context-memory.binding.v1".to_owned(),
        binding_id: request.binding_id,
        phase2_revision_id: committed.state().active_revision_id.clone(),
        phase2_preview_id: preview.preview_id.clone(),
        policy_id: request.policy_id,
        policy_version: request.policy_version,
        selection_sha256: request.selection_sha256,
        audit_sha256: request.audit_sha256,
    };
    let path = env::temp_dir().join(format!(
        "ascension-context-console-phase3-adapter-{}.sqlite",
        std::process::id()
    ));
    let mut store = DurableControlStore::create(&path, STORE_KEY, &paused).map_err(store_error)?;
    let result = store
        .persist_with_memory_binding(&committed, &binding)
        .map_err(store_error)
        .and_then(|()| {
            let stored = store
                .memory_binding(&binding.binding_id)
                .map_err(store_error)?;
            (stored == Some(binding.clone()))
                .then_some(())
                .ok_or_else(|| "phase3-adapter: binding readback mismatch".to_owned())
        });
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = std::fs::remove_file(path.with_extension("sqlite-shm"));
    result?;

    println!(
        "{}",
        serde_json::to_string(&json!({
            "schema": PHASE3_ADAPTER_SCHEMA,
            "status": "completed",
            "selection_id": request.selection_id,
            "review_id": request.review_id,
            "summary_output_sha256": request.summary_output_sha256,
            "first_input_sha256": request.first_input_sha256,
            "phase2_preview_id": preview.preview_id,
            "phase2_revision_id": committed.state().active_revision_id,
            "prepared_manifest_sha256": preview.prepared_manifest_sha256,
            "paused_after_commit": committed.state().pause_latched,
            "resume_required": true,
            "provider_calls": 0,
            "game_launches": 0,
            "external_requests": 0,
            "binding_committed_atomically": true,
            "receipt": receipt,
        }))
        .map_err(encode)?
    );
    Ok(())
}

fn wire_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && b"._:-".contains(&byte))
        })
}

fn wire_digest(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

fn open_store(path: &Path) -> Result<(DurableControlStore, ControlPlane), String> {
    let store = DurableControlStore::open(path, STORE_KEY, RUN_ID).map_err(store_error)?;
    let plane = store.load_for_operator().map_err(store_error)?;
    Ok((store, plane))
}

fn mutate<T: Serialize>(
    store: &mut DurableControlStore,
    plane: ControlPlane,
    operation: &str,
    mutation: impl FnOnce(&mut ControlPlane) -> Result<T, ControlError>,
) -> Result<(), String> {
    let mut candidate = plane;
    let value = mutation(&mut candidate).map_err(control_error)?;
    store.persist(&candidate).map_err(store_error)?;
    let value = serde_json::to_value(value).map_err(encode)?;
    emit(operation, response_status(&value), value)
}

fn capabilities_value(plane: &ControlPlane) -> Value {
    let mut capabilities = plane.capabilities();
    capabilities.durable_control_store = "supported".to_owned();
    serde_json::to_value(capabilities).unwrap_or_else(|_| json!({}))
}

fn response_status(value: &Value) -> &'static str {
    match value
        .get("effect")
        .and_then(Value::as_str)
        .or_else(|| value.get("status").and_then(Value::as_str))
    {
        Some("pause_requested") | Some("resume_accepted") => "accepted",
        Some("revision_committed") | Some("no_change") => "completed",
        _ => "completed",
    }
}

fn command_for(
    plane: &ControlPlane,
    kind: &str,
    idempotency_key: String,
    expected_active_revision_id: Option<String>,
    expected_preview_id: Option<String>,
    approved_manifest_sha256: Option<String>,
) -> ControlCommand {
    ControlCommand {
        schema: "ascension.context-control.command.v1".to_owned(),
        scope: plane.scope().clone(),
        idempotency_key,
        command_window_id: plane.state().command_window_id,
        expected_control_version: plane.state().control_version,
        kind: kind.to_owned(),
        expected_active_revision_id,
        preview_id: expected_preview_id.clone(),
        approved_manifest_sha256,
        expected_preview_id,
    }
}

fn required_path(arguments: &mut impl Iterator<Item = String>) -> Result<PathBuf, String> {
    Ok(PathBuf::from(required_argument(arguments, "store_path")?))
}

fn required_argument(
    arguments: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<String, String> {
    arguments
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing {name}"))
}

fn ensure_no_arguments(
    arguments: &mut impl Iterator<Item = String>,
    operation: &str,
) -> Result<(), String> {
    if arguments.next().is_some() {
        return cli_error(
            operation,
            "unexpected_argument",
            "unexpected trailing argument",
        );
    }
    Ok(())
}

fn parse_u64(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<u64, String> {
    required_argument(arguments, name)?
        .parse()
        .map_err(|_| format!("invalid {name}"))
}

fn parse_bool(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<bool, String> {
    match required_argument(arguments, name)?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("invalid {name}")),
    }
}

fn parse_optional_bool(
    arguments: &mut impl Iterator<Item = String>,
    name: &str,
) -> Result<bool, String> {
    match arguments.next().as_deref() {
        None => Ok(false),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(format!("invalid {name}")),
    }
}

fn stdin_bytes() -> Result<Vec<u8>, String> {
    stdin_bytes_bounded(MAX_PATCH_BYTES)
}

fn stdin_bytes_bounded(limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    io::stdin()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read stdin".to_owned())?;
    if bytes.len() > limit {
        return Err(format!("body_too_large: input exceeds {limit} bytes"));
    }
    Ok(bytes)
}

fn emit<T: Serialize>(operation: &str, status: &str, value: T) -> Result<(), String> {
    let output =
        json!({"schema": CLI_SCHEMA, "operation": operation, "status": status, "value": value});
    println!("{}", serde_json::to_string(&output).map_err(encode)?);
    Ok(())
}

fn cli_error(operation: &str, code: &str, message: &str) -> Result<(), String> {
    Err(format!("{operation}: {code}: {message}"))
}

fn control_error(error: ControlError) -> String {
    error.to_string()
}

fn store_error(error: crate::DurableStoreError) -> String {
    error.to_string()
}

fn encode(_: serde_json::Error) -> String {
    "encoding failed".to_owned()
}

fn print_help() {
    println!("phase2-cli <operation> <store> [arguments]");
    println!("  init <store>");
    println!("  state|capabilities|eligible [--include-content]|revisions|drafts|events <store>");
    println!("  draft-create <store> | draft-show <store> <draft_id>");
    println!("  draft-edit <store> [author]   (typed patch JSON from stdin)");
    println!("  preview <store> <draft_id> <version> <applicable> [risk_ack]");
    println!("  preview-show <store> <preview_id> | command <store> <command_id>");
    println!("  pause <store> <idempotency_key> | commit <store> <key> <preview_id>");
    println!("  resume <store> <idempotency_key> <preview_id>");
    println!(
        "default output is metadata-only JSON; content requires CONTEXT_CONSOLE_CONTENT_TOKEN"
    );
    println!(
        "objective edits require CONTEXT_CONSOLE_OBJECTIVE_TOKEN; this fixture accepts no provider/game effects"
    );
}
