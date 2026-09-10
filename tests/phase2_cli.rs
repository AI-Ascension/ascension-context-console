// SPDX-License-Identifier: MIT

#![allow(clippy::expect_used, clippy::unwrap_used)]

use context_service::{ControlPlane, DurableControlStore, run_phase2_cli};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

fn path() -> PathBuf {
    static NEXT_PATH: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "ascension-context-console-cli-{}-{}.sqlite",
        std::process::id(),
        NEXT_PATH.fetch_add(1, Ordering::Relaxed)
    ))
}

fn cleanup(path: &PathBuf) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("sqlite-wal"));
    let _ = fs::remove_file(path.with_extension("sqlite-shm"));
}

fn cli_process(arguments: &[&str], input: Option<&str>) -> Output {
    cli_process_with_env(arguments, input, &[])
}

fn cli_process_with_env(
    arguments: &[&str],
    input: Option<&str>,
    environment: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_context-console"));
    command.arg("phase2-cli").args(arguments);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in environment {
        command.env(key, value);
    }
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().expect("spawn context-console");
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write patch");
    }
    child.wait_with_output().expect("wait context-console")
}

fn json_output(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("CLI JSON output")
}

#[test]
fn cli_uses_durable_reducer_for_typed_command_sequence() {
    let path = path();
    cleanup(&path);
    let path_text = path.to_string_lossy().into_owned();
    run_phase2_cli(vec!["init".to_owned(), path_text.clone()]).expect("init");
    run_phase2_cli(vec!["draft-create".to_owned(), path_text.clone()]).expect("draft create");
    let store = DurableControlStore::open(&path, [0x42; 32], "fixture-run").expect("open");
    let plane = store.load().expect("load");
    assert_eq!(plane.drafts().len(), 1);
    assert_eq!(plane.capabilities().durable_control_store, "unverified");
    drop(store);
    cleanup(&path);
}

#[test]
fn cli_init_refuses_overwrite_and_help_is_bounded() {
    let path = path();
    cleanup(&path);
    let path_text = path.to_string_lossy().into_owned();
    run_phase2_cli(vec!["init".to_owned(), path_text.clone()]).expect("init");
    assert!(run_phase2_cli(vec!["init".to_owned(), path_text]).is_err());
    run_phase2_cli(vec!["help".to_owned()]).expect("help");
    cleanup(&path);
}

#[test]
fn cli_store_starts_from_the_same_synthetic_scope() {
    let plane = ControlPlane::synthetic();
    assert_eq!(plane.scope().run_id, "fixture-run");
}

#[test]
fn compiled_cli_runs_the_durable_edit_preview_commit_resume_sequence() {
    let path = path();
    cleanup(&path);
    let path_text = path.to_string_lossy().into_owned();
    let init = json_output(cli_process(&["init", &path_text], None));
    assert_eq!(init["schema"], "ascension.context-control.cli-result.v1");
    let capabilities = json_output(cli_process(&["capabilities", &path_text], None));
    assert_eq!(capabilities["value"]["durable_control_store"], "supported");

    let draft = json_output(cli_process(&["draft-create", &path_text], None));
    let draft_value = &draft["value"];
    assert_eq!(draft["status"], "completed");
    let eligible = json_output(cli_process(&["eligible", &path_text], None));
    assert!(
        eligible["value"]["items"]
            .as_array()
            .expect("eligible items")
            .iter()
            .all(|item| item["content"].is_null())
    );
    let denied_content = cli_process(&["eligible", &path_text, "--include-content"], None);
    assert_eq!(denied_content.status.code(), Some(4));
    let permitted_content = json_output(cli_process_with_env(
        &["eligible", &path_text, "--include-content"],
        None,
        &[("CONTEXT_CONSOLE_CONTENT_TOKEN", "fixture-content-token")],
    ));
    assert!(
        permitted_content["value"]["items"]
            .as_array()
            .expect("content items")
            .iter()
            .any(|item| item["content"].is_string())
    );
    let history_item = eligible["value"]["items"]
        .as_array()
        .expect("eligible items")
        .iter()
        .find(|item| item["item"]["item_id"] == "history-1")
        .expect("history item")["item"]
        .clone();
    let patch = serde_json::json!({
        "schema": "ascension.context-control.patch.v1",
        "scope": draft_value["scope"].clone(),
        "draft_id": draft_value["draft_id"].clone(),
        "expected_draft_version": draft_value["version"].clone(),
        "expected_active_revision_id": "revision-1",
        "operations": [{"op": "include_item", "item": history_item}]
    });
    let patch_text = serde_json::to_string(&patch).expect("patch JSON");
    let edited = json_output(cli_process(
        &["draft-edit", &path_text, "operator-cli"],
        Some(&patch_text),
    ));
    assert_eq!(edited["value"]["version"], 2);
    let draft_id = edited["value"]["draft_id"]
        .as_str()
        .expect("draft id")
        .to_owned();

    let paused = json_output(cli_process(&["pause", &path_text, "pause-cli"], None));
    assert_eq!(paused["status"], "accepted");
    let preview = json_output(cli_process(
        &["preview", &path_text, &draft_id, "2", "true", "true"],
        None,
    ));
    assert_eq!(preview["value"]["applicable"], true);
    let preview_id = preview["value"]["preview_id"].as_str().expect("preview id");
    let committed = json_output(cli_process(
        &["commit", &path_text, "commit-cli", preview_id],
        None,
    ));
    assert_eq!(committed["status"], "completed");
    let resumed = json_output(cli_process(
        &["resume", &path_text, "resume-cli", preview_id],
        None,
    ));
    assert_eq!(resumed["status"], "accepted");
    cleanup(&path);
}

#[test]
fn compiled_cli_rejects_duplicate_and_oversized_stdin_without_mutation() {
    let path = path();
    cleanup(&path);
    let path_text = path.to_string_lossy().into_owned();
    let _ = json_output(cli_process(&["init", &path_text], None));
    let _ = json_output(cli_process(&["draft-create", &path_text], None));

    let duplicate = r#"{"schema":"ascension.context-control.patch.v1","schema":"duplicate"}"#;
    let duplicate_result =
        cli_process(&["draft-edit", &path_text, "operator-cli"], Some(duplicate));
    assert_eq!(duplicate_result.status.code(), Some(2));

    let oversized = format!("{{\"payload\":\"{}\"}}", "x".repeat(17 * 1024));
    let oversized_result = cli_process(
        &["draft-edit", &path_text, "operator-cli"],
        Some(&oversized),
    );
    assert_eq!(oversized_result.status.code(), Some(2));
    let drafts = json_output(cli_process(&["drafts", &path_text], None));
    assert_eq!(drafts["value"]["drafts"][0]["version"], 1);
    cleanup(&path);
}

#[test]
fn compiled_cli_distinguishes_missing_durable_store() {
    let path = path();
    cleanup(&path);
    let path_text = path.to_string_lossy().into_owned();
    let result = cli_process(&["state", &path_text], None);
    assert_eq!(result.status.code(), Some(5));
}
