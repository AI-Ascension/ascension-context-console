// SPDX-License-Identifier: MIT

import {
  CONTROL_TOKEN,
  OBJECTIVE_TOKEN,
  controlJson,
  loadEvents,
  loadJson,
  memoryJson,
  sessionJson,
} from "./api.js";
import { loadBundle } from "./bundle.js";
import {
  consoleState,
  invalidatePreview,
  itemKey,
  render,
  renderControl,
  renderDraft,
  renderMemory,
  renderPreview,
  renderProviderSessions,
  setDraftMessage,
  showError,
  validateSnapshot,
} from "./render.js";

let approvedContinuationPreviewId;
let commandCounter = 0;

function scopeForControl() {
  return consoleState.controlState && consoleState.controlState.scope;
}

function command(kind, extra = {}) {
  commandCounter += 1;
  return {
    schema: "ascension.context-control.command.v1",
    scope: scopeForControl(),
    idempotency_key: `browser-${kind}-${commandCounter}`,
    command_window_id: consoleState.controlState.command_window_id,
    expected_control_version: consoleState.controlState.control_version,
    kind,
    ...extra,
  };
}

function selectedItems() {
  return [...document.querySelectorAll("#eligible-rows input.context-select:checked")]
    .map((input) => JSON.parse(input.dataset.item));
}

function pinnedItems() {
  return [...document.querySelectorAll("#eligible-rows input.context-pin:checked")]
    .map((input) => JSON.parse(input.dataset.item));
}

async function searchMemory(event) {
  event.preventDefault();
  const query = document.querySelector("#memory-query").value;
  const body = {
    schema: "ascension.context-memory.query.v1",
    scope: scopeForControl(),
    branch_id: "branch-a",
    query,
    cutoff: 10,
    corpus_generation: 10,
    ranker_version: "lexical-v1",
    limit: 8,
    max_candidates: 64,
    effect_class: "local_read_no_inference",
  };
  try {
    const value = await memoryJson("/search", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    document.querySelector("#memory-results").textContent = JSON.stringify({
      coverage: value.coverage,
      results: value.results,
      inference_calls: value.inference_calls,
    }, null, 2);
  } catch (error) {
    document.querySelector("#memory-results").textContent = error instanceof Error ? error.message : "search unavailable";
  }
}

async function loadMemory() {
  try {
    const [capabilities, memoryStatus] = await Promise.all([
      memoryJson("/capabilities"),
      memoryJson("/status"),
    ]);
    renderMemory(capabilities, memoryStatus);
    document.querySelector("#memory-search-form").addEventListener("submit", searchMemory);
  } catch (error) {
    document.querySelector("#memory-panel").hidden = false;
    document.querySelector("#compaction-panel").hidden = false;
    document.querySelector("#memory-message").textContent = error instanceof Error ? error.message : "memory facade unavailable";
  }
}

async function refreshProviderSessions() {
  const [capabilities, list] = await Promise.all([
    sessionJson("/provider-sessions/capabilities"),
    sessionJson("/provider-sessions"),
  ]);
  renderProviderSessions(capabilities, list);
}

async function createProviderCandidate() {
  const command = {
    idempotency_key: `browser-candidate-${Date.now()}`,
    expected_control_generation: 0,
    approved_policy_ref: "policy-fixture",
    profile_ref: "profile-fixture",
    purpose: "evaluation",
  };
  try {
    const result = await sessionJson("/provider-sessions/candidates", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(command),
    });
    document.querySelector("#session-message").textContent = `Candidate ${result.operation} accepted locally; native calls=${result.value?.native_calls ?? 0}, game effects=${result.value?.game_effects ?? 0}.`;
    await refreshProviderSessions();
  } catch (error) {
    document.querySelector("#session-message").textContent = error instanceof Error ? error.message : "candidate request failed";
  }
}

async function loadProviderSessions() {
  try {
    await refreshProviderSessions();
    document.querySelector("#session-refresh").addEventListener("click", refreshProviderSessions);
    document.querySelector("#session-create-candidate").addEventListener("click", createProviderCandidate);
  } catch (error) {
    document.querySelector("#session-panel").hidden = false;
    document.querySelector("#session-message").textContent = error instanceof Error ? error.message : "provider session boundary unavailable";
  }
}

async function refreshControl() {
  const [capabilities, stateValue, items, revisions] = await Promise.all([
    controlJson("/capabilities"),
    controlJson("/state"),
    controlJson("/eligible-items"),
    controlJson("/revisions"),
  ]);
  renderControl(capabilities, stateValue, items, revisions);
  return stateValue;
}

async function createDraft() {
  try {
    renderDraft(await controlJson("/drafts", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ scope: scopeForControl(), expected_active_revision_id: consoleState.controlState.active_revision_id }) }));
    setDraftMessage("Draft created in memory-backed fixture control storage.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "draft creation failed", true); }
}

async function saveDraft() {
  if (!consoleState.currentDraft) return;
  const selected = selectedItems();
  const selectedKeys = new Set(selected.map(itemKey));
  const existing = new Set(consoleState.currentDraft.selected_items.map(itemKey));
  const visible = new Map([...document.querySelectorAll("#eligible-rows input.context-select")]
    .map((input) => {
      const item = JSON.parse(input.dataset.item);
      return [item.item_id, item];
    }));
  const operations = selected
    .filter((item) => !existing.has(itemKey(item)))
    .map((item) => ({ op: "include_item", item }));
  const desiredPinned = new Set(pinnedItems().map((item) => item.item_id));
  consoleState.currentDraft.pinned_item_ids
    .filter((itemId) => visible.has(itemId) && !desiredPinned.has(itemId))
    .forEach((itemId) => operations.push({ op: "unpin_item", item: visible.get(itemId) }));
  pinnedItems()
    .filter((item) => !consoleState.currentDraft.pinned_item_ids.includes(item.item_id))
    .forEach((item) => operations.push({ op: "pin_item", item }));
  consoleState.currentDraft.selected_items
    .filter((item) => visible.has(item.item_id) && !selectedKeys.has(itemKey(item)))
    .forEach((item) => operations.push({ op: "exclude_item", item }));
  const note = document.querySelector("#note-text").value;
  const existingNote = consoleState.currentDraft.note_items.find((item) => item.item_id === "note-browser");
  if (note) operations.push({ op: "put_note", note_id: "note-browser", expected_note_version: existingNote?.version ?? null, text: note, expires_at: "2030-01-01T00:00:00Z" });
  const objective = document.querySelector("#objective-text").value;
  if (objective) operations.push({ op: "set_objective", text: objective });
  if (!operations.length) { setDraftMessage("Select an editable item or enter a bounded note first.", true); return; }
  try {
    renderDraft(await controlJson(`/drafts/${consoleState.currentDraft.draft_id}/operations`, { token: objective ? OBJECTIVE_TOKEN : CONTROL_TOKEN, method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ schema: "ascension.context-control.patch.v1", scope: scopeForControl(), draft_id: consoleState.currentDraft.draft_id, expected_draft_version: consoleState.currentDraft.version, expected_active_revision_id: consoleState.controlState.active_revision_id, operations }) }));
    invalidatePreview();
    setDraftMessage("Draft saved; any prior preview is invalid.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "draft save failed", true); }
}

async function restoreDraft() {
  if (!consoleState.currentDraft) { setDraftMessage("Start a draft before restoring a configuration.", true); return; }
  const sourceRevisionId = document.querySelector("#restore-source").value;
  if (!sourceRevisionId) { setDraftMessage("Choose a retained revision to restore.", true); return; }
  try {
    renderDraft(await controlJson(`/drafts/${consoleState.currentDraft.draft_id}/operations`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema: "ascension.context-control.patch.v1",
        scope: scopeForControl(),
        draft_id: consoleState.currentDraft.draft_id,
        expected_draft_version: consoleState.currentDraft.version,
        expected_active_revision_id: consoleState.controlState.active_revision_id,
        operations: [{ op: "restore_configuration", source_revision_id: sourceRevisionId }],
      }),
    }));
    document.querySelector("#note-text").value = "";
    document.querySelector("#objective-text").value = "";
    invalidatePreview();
    setDraftMessage("Configuration restored into the draft; preview it again before commit.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "restore failed", true); }
}

async function removeNote() {
  if (!consoleState.currentDraft) return;
  const note = consoleState.currentDraft.note_items.find((item) => item.item_id === "note-browser");
  if (!note) return;
  try {
    renderDraft(await controlJson(`/drafts/${consoleState.currentDraft.draft_id}/operations`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema: "ascension.context-control.patch.v1",
        scope: scopeForControl(),
        draft_id: consoleState.currentDraft.draft_id,
        expected_draft_version: consoleState.currentDraft.version,
        expected_active_revision_id: consoleState.controlState.active_revision_id,
        operations: [{ op: "remove_note", note_id: note.item_id, expected_note_version: note.version }],
      }),
    }));
    document.querySelector("#note-text").value = "";
    invalidatePreview();
    setDraftMessage("Operator note removed from the draft; preview it again before commit.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "note removal failed", true); }
}

async function makePreview() {
  if (!consoleState.currentDraft) { setDraftMessage("Start a draft before previewing.", true); return; }
  try {
    const preview = await controlJson("/previews", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ scope: scopeForControl(), draft_id: consoleState.currentDraft.draft_id, expected_draft_version: consoleState.currentDraft.version, applicable_requested: Boolean(consoleState.controlState.pause_latched), expected_control_version: consoleState.controlState.control_version, unknown_total_risk_acknowledged: document.querySelector("#budget-risk-ack").checked }) });
    renderPreview(preview);
    setDraftMessage(preview.applicable ? "Applicable preview is frozen for commit." : `Exploratory preview: ${preview.blockers.join(", ") || "run is not held"}.`);
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "preview failed", true); }
}

async function pauseRun() {
  try {
    const receipt = await controlJson("/pause", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command("pause")) });
    await refreshControl();
    setDraftMessage(`Pause ${receipt.status}: the scheduler remains held until explicit resume.`);
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "pause failed", true); }
}

async function commitDraft() {
  if (!consoleState.currentPreview || !consoleState.currentPreview.applicable) return;
  try {
    const receipt = await controlJson("/commits", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command("commit", { expected_active_revision_id: consoleState.controlState.active_revision_id, preview_id: consoleState.currentPreview.preview_id, approved_manifest_sha256: consoleState.currentPreview.prepared_manifest_sha256 })) });
    approvedContinuationPreviewId = consoleState.currentPreview.preview_id;
    invalidatePreview();
    await refreshControl();
    setDraftMessage(`Commit ${receipt.status}: revision ${receipt.active_revision_id} is committed while paused.`);
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "commit failed", true); }
}

async function resumeRun() {
  try {
    const receipt = await controlJson("/resume", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command("resume", { expected_active_revision_id: consoleState.controlState.active_revision_id, expected_preview_id: approvedContinuationPreviewId || (consoleState.currentPreview?.applicable ? consoleState.currentPreview.preview_id : null) })) });
    approvedContinuationPreviewId = null;
    await refreshControl();
    setDraftMessage(`Resume ${receipt.status}: no automatic provider or game call was made by the console.`);
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "resume failed", true); }
}

async function loadControl() {
  try {
    await refreshControl();
    document.querySelector("#create-draft").addEventListener("click", createDraft);
    document.querySelector("#save-draft").addEventListener("click", saveDraft);
    document.querySelector("#preview-draft").addEventListener("click", makePreview);
    document.querySelector("#pause-run").addEventListener("click", pauseRun);
    document.querySelector("#commit-draft").addEventListener("click", commitDraft);
    document.querySelector("#resume-run").addEventListener("click", resumeRun);
    document.querySelector("#restore-draft").addEventListener("click", restoreDraft);
    document.querySelector("#remove-note").addEventListener("click", removeNote);
  } catch (error) {
    document.querySelector("#control-message").textContent = error instanceof Error ? error.message : "management control is unavailable";
  }
}

async function loadFixture() {
  const bundle = await loadBundle();
  const [snapshot, events] = await Promise.all([loadJson(bundle.snapshotUrl), loadEvents(bundle.eventsUrl)]);
  const rendered = render(snapshot, events);
  const comparison = await loadJson(bundle.compareUrl);
  validateSnapshot(comparison);
  document.querySelector("#compare-button").addEventListener("click", () => {
    const left = rendered.components.map((component) => `${component.component_id}:${component.observed_bytes}`).join("|");
    const right = comparison.components.map((component) => `${component.component_id}:${component.observed_bytes}`).join("|");
    document.querySelector("#comparison-result").textContent = left === right
      ? "The retained snapshots have the same ordered component measurements. This view does not imply a cache hit."
      : `The retained snapshots differ at the application boundary (${rendered.capture_mode} vs ${comparison.capture_mode}); no future input was changed.`;
  });
  await loadControl();
  await loadMemory();
  await loadProviderSessions();
}

loadFixture().catch((error) => {
  showError(error instanceof Error ? error.message : "bundle could not be read");
});
