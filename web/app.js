// SPDX-License-Identifier: MIT

const MAX_FIXTURE_BYTES = 1024 * 1024;
const MAX_EVENTS = 256;
const bundleManifestUrl = new URL("../offline-bundle.json", document.baseURI);
const CONTROL_BASE = "/v2/runs/fixture-run/context-control";
const MEMORY_BASE = "/v3/memory";
const SESSION_BASE = "/v1/runs/fixture-run-001";
const CONTROL_TOKEN = "fixture-editor-token";
const OBJECTIVE_TOKEN = "fixture-objective-token";
const SESSION_TOKEN = "fixture-session-token";
const CSRF_TOKEN = "fixture-csrf-token";
const status = document.querySelector("#status");
let controlState;
let controlCapabilities;
let currentDraft;
let currentPreview;
let approvedContinuationPreviewId;
let commandCounter = 0;

function showText(selector, value) {
  const node = document.querySelector(selector);
  node.textContent = value == null || value === "" ? "unavailable" : String(value);
}

function showError(message) {
  status.textContent = "Offline bundle unavailable";
  status.dataset.state = "error";
  document.querySelector("#error-message").textContent = message;
  document.querySelector("#error").hidden = false;
}

function validateSnapshot(snapshot) {
  if (!snapshot || typeof snapshot !== "object") throw new Error("snapshot is not an object");
  if (snapshot.schema !== "ascension.context-snapshot.v1") throw new Error("unsupported snapshot schema");
  if (!Array.isArray(snapshot.components) || snapshot.components.length === 0 || snapshot.components.length > 128) {
    throw new Error("component list exceeds its bound");
  }
  if (!Array.isArray(snapshot.mapping) || snapshot.mapping.length > 128) throw new Error("mapping list exceeds its bound");
  if (!snapshot.provider || snapshot.provider.additional_context !== "not_exposed") {
    throw new Error("provider context is not classified");
  }
  const ids = new Set();
  snapshot.components.forEach((component, index) => {
    if (component.ordinal !== index || ids.has(component.component_id)) throw new Error("component order is invalid");
    ids.add(component.component_id);
  });
  snapshot.mapping.forEach((mapping) => mapping.component_ids.forEach((id) => {
    if (!ids.has(id)) throw new Error("mapping references an unknown component");
  }));
}

function validateEvents(events) {
  if (!Array.isArray(events) || events.length > MAX_EVENTS) throw new Error("event stream exceeds its bound");
  events.forEach((event) => {
    if (event.schema !== "ascension.context-event.v1" || !event.event_id || !event.event_type) {
      throw new Error("event is invalid");
    }
    if (event.details && Object.keys(event.details).some((key) => key.includes("reasoning") || key.includes("raw_"))) {
      throw new Error("event contains a forbidden content field");
    }
  });
}

function resolveBundleArtifact(value, field) {
  const hasControlOrWhitespace = typeof value === "string"
    && [...value].some((character) => {
      const codePoint = character.codePointAt(0);
      return codePoint <= 0x20 || codePoint === 0x7f;
    });
  if (typeof value !== "string" || value.length === 0 || value.length > 256
    || hasControlOrWhitespace || value.startsWith("/") || value.includes("\\") || value.includes("..")
    || value.includes("%") || value.includes("://")) {
    throw new Error(`offline bundle ${field} path is invalid`);
  }
  const url = new URL(value, bundleManifestUrl);
  if (url.origin !== window.location.origin || url.search || url.hash) {
    throw new Error(`offline bundle ${field} URL is invalid`);
  }
  return url;
}

function validateBundleManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || manifest.schema !== "ascension.offline-bundle.v1"
    || manifest.evidence !== "synthetic") {
    throw new Error("unsupported offline bundle manifest");
  }
  return {
    snapshotUrl: resolveBundleArtifact(manifest.snapshot, "snapshot"),
    compareUrl: resolveBundleArtifact(manifest.comparison, "comparison"),
    eventsUrl: resolveBundleArtifact(manifest.events, "events"),
  };
}

function render(snapshot, events) {
  validateSnapshot(snapshot);
  validateEvents(events);
  const complete = snapshot.application_capture_complete === true;
  showText("#snapshot-id", snapshot.snapshot_id);
  showText("#boundary", snapshot.boundary);
  showText("#provider-model", snapshot.provider.model);
  showText("#recorded-at", snapshot.recorded_at);
  showText("#component-count", snapshot.components.length);
  showText("#evidence", snapshot.producer && snapshot.producer.evidence);
  showText("#model-limit", snapshot.model_context_limit_tokens == null
    ? snapshot.model_limit_source
    : `${snapshot.model_context_limit_tokens} tokens (${snapshot.model_limit_source})`);
  const measurement = snapshot.input_measurement || {};
  showText("#measurement", measurement.value == null ? measurement.source : `${measurement.value} (${measurement.source})`);

  const badge = document.querySelector("#capture-badge");
  badge.textContent = complete ? "complete at boundary" : "partial at boundary";
  badge.dataset.state = complete ? "complete" : "partial";
  const reasons = Array.isArray(snapshot.incomplete_reasons) && snapshot.incomplete_reasons.length
    ? snapshot.incomplete_reasons.join(", ")
    : "unavailable";
  showText("#incomplete", complete
    ? "All declared application components were captured for this boundary."
    : `The producer marked this snapshot incomplete: ${reasons}.`);

  const rows = document.querySelector("#component-rows");
  rows.replaceChildren();
  snapshot.components.forEach((component) => {
    const row = document.createElement("tr");
    [component.ordinal, component.kind, component.observed_bytes, component.content_status]
      .map((value) => value == null ? "unavailable" : String(value))
      .forEach((value) => {
        const cell = document.createElement("td");
        cell.textContent = value;
        row.append(cell);
      });
    rows.append(row);
  });

  const mappingList = document.querySelector("#mapping-list");
  mappingList.replaceChildren();
  snapshot.mapping.forEach((mapping) => {
    const item = document.createElement("li");
    const components = mapping.component_ids.length ? mapping.component_ids.join(", ") : "no component";
    item.textContent = `${mapping.upstream_field}: ${mapping.transformation} (${components})`;
    mappingList.append(item);
  });

  const timeline = document.querySelector("#timeline-list");
  timeline.replaceChildren();
  events.slice().sort((left, right) => left.sequence - right.sequence).forEach((event) => {
    const item = document.createElement("li");
    item.textContent = `#${event.sequence}: ${event.event_type} · ${event.observed_at}`;
    timeline.append(item);
  });

  document.querySelector("#summary").hidden = false;
  document.querySelector("#components").hidden = false;
  document.querySelector("#mapping").hidden = false;
  document.querySelector("#timeline").hidden = false;
  document.querySelector("#comparison").hidden = false;
  status.textContent = "Synthetic offline bundle loaded";
  return snapshot;
}

async function boundedText(response) {
  if (!response.ok) throw new Error(`bundle request returned ${response.status}`);
  const text = await response.text();
  if (new TextEncoder().encode(text).length > MAX_FIXTURE_BYTES) throw new Error("bundle exceeds its byte bound");
  return text;
}

async function loadJson(url) {
  return JSON.parse(await boundedText(await fetch(url, { cache: "no-store" })));
}

async function loadEvents(eventsUrl) {
  const text = await boundedText(await fetch(eventsUrl, { cache: "no-store" }));
  return text.split("\n").filter((line) => line.trim()).map((line) => JSON.parse(line));
}

async function controlJson(path, options = {}) {
  const { token = CONTROL_TOKEN, ...requestOptions } = options;
  const headers = new Headers(requestOptions.headers || {});
  headers.set("Authorization", `Bearer ${token}`);
  if (requestOptions.method && requestOptions.method !== "GET") headers.set("X-CSRF-Token", CSRF_TOKEN);
  const response = await fetch(`${CONTROL_BASE}${path}`, { ...requestOptions, headers, cache: "no-store" });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error?.message || value.error?.code || `control request returned ${response.status}`);
  return value;
}

async function memoryJson(path, options = {}) {
  const { token = CONTROL_TOKEN, ...requestOptions } = options;
  const headers = new Headers(requestOptions.headers || {});
  headers.set("Authorization", `Bearer ${token}`);
  if (requestOptions.method && requestOptions.method !== "GET") headers.set("X-CSRF-Token", CSRF_TOKEN);
  const response = await fetch(`${MEMORY_BASE}${path}`, { ...requestOptions, headers, cache: "no-store" });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || `memory request returned ${response.status}`);
  return value;
}

function renderMemory(capabilities, memoryStatus) {
  showText("#memory-badge", capabilities.enabled ? "enabled" : "disabled");
  showText("#memory-capability", capabilities.enabled ? "enabled" : "disabled by default");
  showText("#memory-retrieval", capabilities.local_lexical_retrieval);
  showText("#memory-compaction", `${capabilities.extractive_compaction} · adapter ${capabilities.abstractive_adapter}`);
  showText("#memory-approval", capabilities.phase2_approval_required ? "Phase 2 approval required" : "unavailable");
  showText("#memory-generation", memoryStatus.corpus_generation);
  showText("#memory-revocation", memoryStatus.revocation_epoch);
  document.querySelector("#memory-panel").hidden = false;
  document.querySelector("#compaction-panel").hidden = false;
  const message = document.querySelector("#memory-message");
  message.textContent = capabilities.enabled
    ? "Lexical search is scoped to the pinned corpus and cutoff; reads do not call a provider."
    : "Memory is disabled by default; no corpus is created and no inference is available.";
  document.querySelector("#memory-search-button").disabled = !capabilities.enabled;
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

async function sessionJson(path, options = {}) {
  const { token = SESSION_TOKEN, ...requestOptions } = options;
  const headers = new Headers(requestOptions.headers || {});
  headers.set("Authorization", `Bearer ${token}`);
  if (requestOptions.method && requestOptions.method !== "GET") {
    headers.set("X-CSRF-Token", CSRF_TOKEN);
  }
  const response = await fetch(`${SESSION_BASE}${path}`, { ...requestOptions, headers, cache: "no-store" });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || `provider session request returned ${response.status}`);
  return value;
}

function renderProviderSessions(capabilitiesEnvelope, listEnvelope) {
  const capabilities = capabilitiesEnvelope.value || {};
  const list = listEnvelope.value || {};
  showText("#session-badge", capabilitiesEnvelope.operation === "capabilities" ? "fixture-only" : "available");
  showText("#session-mode", "fixture_only");
  showText("#session-profile", capabilities.profile_id);
  showText("#session-evidence", capabilities.evidence);
  showText("#session-transport", capabilities.transport);
  showText("#session-hardening", capabilities.hardening
    ? `tools=${capabilities.hardening.tools_enabled ? "on" : "off"} · ambient=${capabilities.hardening.ambient_history ? "on" : "off"}`
    : "unavailable");
  showText("#session-method-count", Array.isArray(capabilities.enabled_methods) ? capabilities.enabled_methods.length : 0);
  document.querySelector("#session-methods").textContent = JSON.stringify({
    enabled_methods: capabilities.enabled_methods || [],
    unknown_methods: capabilities.unknown_methods,
    raw_rpc: capabilities.raw_rpc,
    native_calls: 0,
    game_effects: 0,
  }, null, 2);
  const rows = document.querySelector("#session-rows");
  rows.replaceChildren();
  (list.bindings || []).forEach((binding) => {
    const row = document.createElement("tr");
    [binding.binding_id, binding.state, binding.purpose, binding.history_coverage, binding.game_dispatch_capability ? "enabled" : "disabled"]
      .forEach((value) => { const cell = document.createElement("td"); cell.textContent = String(value ?? "unavailable"); row.append(cell); });
    rows.append(row);
  });
  document.querySelector("#session-panel").hidden = false;
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

function setManagementControls(enabled) {
  ["create-draft", "save-draft", "preview-draft", "pause-run", "commit-draft", "resume-run", "restore-draft", "remove-note"]
    .forEach((id) => { document.querySelector(`#${id}`).disabled = !enabled; });
}

function syncDraftSelections() {
  if (!currentDraft) return;
  const selected = new Set(currentDraft.selected_items.map(itemKey));
  const pinned = new Set(currentDraft.pinned_item_ids);
  document.querySelectorAll("#eligible-rows input.context-select").forEach((input) => {
    input.checked = selected.has(input.dataset.item && itemKey(JSON.parse(input.dataset.item)));
  });
  document.querySelectorAll("#eligible-rows input.context-pin").forEach((input) => {
    const item = JSON.parse(input.dataset.item);
    const selectedItem = selected.has(itemKey(item));
    input.checked = selectedItem && pinned.has(item.item_id);
    input.disabled = !selectedItem;
  });
}

function renderControl(capabilities, stateValue, items, revisions) {
  controlCapabilities = capabilities;
  controlState = stateValue;
  showText("#management-badge", capabilities.enabled ? "management enabled" : "legacy mode");
  showText("#control-status", stateValue.status);
  showText("#active-revision", stateValue.active_revision_id);
  showText("#control-version", stateValue.control_version);
  showText("#pause-latch", stateValue.pause_latched ? "latched" : "open");
  showText("#plan-epoch", stateValue.plan_epoch);
  showText("#durable-store", capabilities.durable_control_store);
  const rows = document.querySelector("#eligible-rows");
  rows.replaceChildren();
  items.items.forEach((entry) => {
    const row = document.createElement("tr");
    const useCell = document.createElement("td");
    const pinCell = document.createElement("td");
    if (entry.protected || !entry.content_available) {
      useCell.textContent = "locked";
      pinCell.textContent = "locked";
    } else {
      const useInput = document.createElement("input");
      useInput.type = "checkbox";
      useInput.className = "context-select";
      useInput.dataset.item = JSON.stringify(entry.item);
      useInput.setAttribute("aria-label", `Include ${entry.item.item_id}`);
      const pinInput = document.createElement("input");
      pinInput.type = "checkbox";
      pinInput.className = "context-pin";
      pinInput.dataset.item = JSON.stringify(entry.item);
      pinInput.setAttribute("aria-label", `Pin ${entry.item.item_id}`);
      useInput.addEventListener("change", () => {
        pinInput.disabled = !useInput.checked;
        if (!useInput.checked) pinInput.checked = false;
      });
      useCell.append(useInput);
      pinCell.append(pinInput);
    }
    row.append(useCell);
    row.append(pinCell);
    [entry.item.item_id, entry.kind, entry.item.version, entry.protected ? (entry.locked_reason || "protected") : "editable"]
      .forEach((value) => { const cell = document.createElement("td"); cell.textContent = String(value); row.append(cell); });
    rows.append(row);
  });
  const restoreSource = document.querySelector("#restore-source");
  restoreSource.replaceChildren();
  revisions.revisions.slice().sort((left, right) => right.sequence - left.sequence).forEach((revision) => {
    const option = document.createElement("option");
    option.value = revision.revision_id;
    option.textContent = `${revision.revision_id} · ${revision.state_after_commit}`;
    restoreSource.append(option);
  });
  setManagementControls(capabilities.enabled);
  if (!currentDraft) {
    document.querySelector("#save-draft").disabled = true;
    document.querySelector("#preview-draft").disabled = true;
    document.querySelector("#restore-draft").disabled = true;
    document.querySelector("#remove-note").disabled = true;
  }
  document.querySelector("#commit-draft").disabled = !currentPreview?.applicable || !capabilities.enabled;
  syncDraftSelections();
  document.querySelector("#control-panel").hidden = false;
  document.querySelector("#eligible").hidden = false;
  document.querySelector("#editor").hidden = false;
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

function scopeForControl() {
  return controlState && controlState.scope;
}

function setDraftMessage(message, isError = false) {
  const node = document.querySelector("#draft-message");
  node.textContent = message;
  node.dataset.state = isError ? "error" : "";
}

function command(kind, extra = {}) {
  commandCounter += 1;
  return {
    schema: "ascension.context-control.command.v1",
    scope: scopeForControl(),
    idempotency_key: `browser-${kind}-${commandCounter}`,
    command_window_id: controlState.command_window_id,
    expected_control_version: controlState.control_version,
    kind,
    ...extra,
  };
}

function renderDraft(draft) {
  currentDraft = draft;
  showText("#draft-version", `Draft ${draft.draft_id} · version ${draft.version}`);
  document.querySelector("#save-draft").disabled = !controlCapabilities?.enabled;
  document.querySelector("#preview-draft").disabled = !controlCapabilities?.enabled;
  document.querySelector("#restore-draft").disabled = !controlCapabilities?.enabled;
  document.querySelector("#remove-note").disabled = !controlCapabilities?.enabled
    || !draft.note_items.some((item) => item.item_id === "note-browser");
  syncDraftSelections();
}

function renderPreview(preview) {
  currentPreview = preview;
  showText("#preview-id", preview.preview_id);
  showText("#prepared-digest", preview.prepared_manifest_sha256);
  showText("#preview-components", preview.components.length ? preview.components.map((item) => `${item.kind}:${item.bytes} B`).join(", ") : "none");
  showText("#preview-budget", `${preview.budget_status}${preview.unknown_total_risk_acknowledged ? " · risk acknowledged" : ""}`);
  showText("#preview-provider-context", preview.provider_added_context);
  showText("#preview-badge", preview.applicable ? "applicable" : "exploratory / blocked");
  const diff = {
    blockers: preview.blockers,
    selected_items: preview.selected_items,
    notes: currentDraft?.note_items ?? [],
    objective: currentDraft?.objective_item ?? null,
    components: preview.components,
  };
  document.querySelector("#preview-diff").textContent = JSON.stringify(diff, null, 2);
  document.querySelector("#preview").hidden = false;
  document.querySelector("#commit-draft").disabled = !preview.applicable;
}

function invalidatePreview() {
  currentPreview = null;
  document.querySelector("#preview").hidden = true;
  document.querySelector("#preview-id").textContent = "";
  document.querySelector("#prepared-digest").textContent = "";
  document.querySelector("#preview-diff").textContent = "";
  document.querySelector("#commit-draft").disabled = true;
}

async function createDraft() {
  try {
    renderDraft(await controlJson("/drafts", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ scope: scopeForControl(), expected_active_revision_id: controlState.active_revision_id }) }));
    setDraftMessage("Draft created in memory-backed fixture control storage.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "draft creation failed", true); }
}

function selectedItems() {
  return [...document.querySelectorAll("#eligible-rows input.context-select:checked")]
    .map((input) => JSON.parse(input.dataset.item));
}

function pinnedItems() {
  return [...document.querySelectorAll("#eligible-rows input.context-pin:checked")]
    .map((input) => JSON.parse(input.dataset.item));
}

function itemKey(item) {
  return `${item.item_id}:${item.version}:${item.sha256}`;
}

async function saveDraft() {
  if (!currentDraft) return;
  const selected = selectedItems();
  const selectedKeys = new Set(selected.map(itemKey));
  const existing = new Set(currentDraft.selected_items.map(itemKey));
  const visible = new Map([...document.querySelectorAll("#eligible-rows input.context-select")]
    .map((input) => {
      const item = JSON.parse(input.dataset.item);
      return [item.item_id, item];
    }));
  const operations = selected
    .filter((item) => !existing.has(itemKey(item)))
    .map((item) => ({ op: "include_item", item }));
  const desiredPinned = new Set(pinnedItems().map((item) => item.item_id));
  currentDraft.pinned_item_ids
    .filter((itemId) => visible.has(itemId) && !desiredPinned.has(itemId))
    .forEach((itemId) => operations.push({ op: "unpin_item", item: visible.get(itemId) }));
  pinnedItems()
    .filter((item) => !currentDraft.pinned_item_ids.includes(item.item_id))
    .forEach((item) => operations.push({ op: "pin_item", item }));
  currentDraft.selected_items
    .filter((item) => visible.has(item.item_id) && !selectedKeys.has(itemKey(item)))
    .forEach((item) => operations.push({ op: "exclude_item", item }));
  const note = document.querySelector("#note-text").value;
  const existingNote = currentDraft.note_items.find((item) => item.item_id === "note-browser");
  if (note) operations.push({ op: "put_note", note_id: "note-browser", expected_note_version: existingNote?.version ?? null, text: note, expires_at: "2030-01-01T00:00:00Z" });
  const objective = document.querySelector("#objective-text").value;
  if (objective) operations.push({ op: "set_objective", text: objective });
  if (!operations.length) { setDraftMessage("Select an editable item or enter a bounded note first.", true); return; }
  try {
    renderDraft(await controlJson(`/drafts/${currentDraft.draft_id}/operations`, { token: objective ? OBJECTIVE_TOKEN : CONTROL_TOKEN, method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ schema: "ascension.context-control.patch.v1", scope: scopeForControl(), draft_id: currentDraft.draft_id, expected_draft_version: currentDraft.version, expected_active_revision_id: controlState.active_revision_id, operations }) }));
    invalidatePreview();
    setDraftMessage("Draft saved; any prior preview is invalid.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "draft save failed", true); }
}

async function restoreDraft() {
  if (!currentDraft) { setDraftMessage("Start a draft before restoring a configuration.", true); return; }
  const sourceRevisionId = document.querySelector("#restore-source").value;
  if (!sourceRevisionId) { setDraftMessage("Choose a retained revision to restore.", true); return; }
  try {
    renderDraft(await controlJson(`/drafts/${currentDraft.draft_id}/operations`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema: "ascension.context-control.patch.v1",
        scope: scopeForControl(),
        draft_id: currentDraft.draft_id,
        expected_draft_version: currentDraft.version,
        expected_active_revision_id: controlState.active_revision_id,
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
  if (!currentDraft) return;
  const note = currentDraft.note_items.find((item) => item.item_id === "note-browser");
  if (!note) return;
  try {
    renderDraft(await controlJson(`/drafts/${currentDraft.draft_id}/operations`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        schema: "ascension.context-control.patch.v1",
        scope: scopeForControl(),
        draft_id: currentDraft.draft_id,
        expected_draft_version: currentDraft.version,
        expected_active_revision_id: controlState.active_revision_id,
        operations: [{ op: "remove_note", note_id: note.item_id, expected_note_version: note.version }],
      }),
    }));
    document.querySelector("#note-text").value = "";
    invalidatePreview();
    setDraftMessage("Operator note removed from the draft; preview it again before commit.");
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "note removal failed", true); }
}

async function makePreview() {
  if (!currentDraft) { setDraftMessage("Start a draft before previewing.", true); return; }
  try {
    const preview = await controlJson("/previews", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ scope: scopeForControl(), draft_id: currentDraft.draft_id, expected_draft_version: currentDraft.version, applicable_requested: Boolean(controlState.pause_latched), expected_control_version: controlState.control_version, unknown_total_risk_acknowledged: document.querySelector("#budget-risk-ack").checked }) });
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
  if (!currentPreview || !currentPreview.applicable) return;
  try {
    const receipt = await controlJson("/commits", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command("commit", { expected_active_revision_id: controlState.active_revision_id, preview_id: currentPreview.preview_id, approved_manifest_sha256: currentPreview.prepared_manifest_sha256 })) });
    approvedContinuationPreviewId = currentPreview.preview_id;
    invalidatePreview();
    await refreshControl();
    setDraftMessage(`Commit ${receipt.status}: revision ${receipt.active_revision_id} is committed while paused.`);
  } catch (error) { setDraftMessage(error instanceof Error ? error.message : "commit failed", true); }
}

async function resumeRun() {
  try {
    const receipt = await controlJson("/resume", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(command("resume", { expected_active_revision_id: controlState.active_revision_id, expected_preview_id: approvedContinuationPreviewId || (currentPreview?.applicable ? currentPreview.preview_id : null) })) });
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
  const bundle = validateBundleManifest(await loadJson(bundleManifestUrl));
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
