// SPDX-License-Identifier: MIT

//! Bounded operator CLI for the synthetic Phase 2 control store.
//!
//! This is intentionally a fixture command surface. It uses the same typed reducer and strict
//! payload parser as the integrated HTTP path, keeps private note text on stdin, and emits metadata
//! by default. A production deployment must replace the fixture key and capability plumbing.

use crate::{
    ControlCommand, ControlError, ControlPatch, ControlPlane, DurableControlStore,
    MAX_MEMORY_BODY_BYTES, MemoryQueryRequest, MemoryRoute, MemoryScope, parse_control_json,
};
use serde::Serialize;
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
    let mut route = MemoryRoute::new(scope, true);
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
