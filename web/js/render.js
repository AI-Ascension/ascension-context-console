// SPDX-License-Identifier: MIT

const MAX_EVENTS = 256;
const status = document.querySelector("#status");

// Shared mutable console state. Renderers publish the latest control, draft, and
// preview envelopes here; the app wiring in app.js reads them back when it builds
// follow-up commands or refreshes the page.
export const consoleState = {
  controlState: undefined,
  currentDraft: undefined,
  currentPreview: undefined,
};

let controlCapabilities;

function showText(selector, value) {
  const node = document.querySelector(selector);
  node.textContent = value == null || value === "" ? "unavailable" : String(value);
}

export function showError(message) {
  status.textContent = "Offline bundle unavailable";
  status.dataset.state = "error";
  document.querySelector("#error-message").textContent = message;
  document.querySelector("#error").hidden = false;
}

export function validateSnapshot(snapshot) {
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

export function render(snapshot, events) {
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
    item.textContent = `#${event.sequence}: ${event.event_type} \u00b7 ${event.observed_at}`;
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

function setManagementControls(enabled) {
  ["create-draft", "save-draft", "preview-draft", "pause-run", "commit-draft", "resume-run", "restore-draft", "remove-note"]
    .forEach((id) => { document.querySelector(`#${id}`).disabled = !enabled; });
}

function syncDraftSelections() {
  if (!consoleState.currentDraft) return;
  const selected = new Set(consoleState.currentDraft.selected_items.map(itemKey));
  const pinned = new Set(consoleState.currentDraft.pinned_item_ids);
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

export function renderMemory(capabilities, memoryStatus) {
  showText("#memory-badge", capabilities.enabled ? "enabled" : "disabled");
  showText("#memory-capability", capabilities.enabled ? "enabled" : "disabled by default");
  showText("#memory-retrieval", capabilities.local_lexical_retrieval);
  showText("#memory-compaction", `${capabilities.extractive_compaction} \u00b7 adapter ${capabilities.abstractive_adapter}`);
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

export function renderControl(capabilities, stateValue, items, revisions) {
  controlCapabilities = capabilities;
  consoleState.controlState = stateValue;
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
    option.textContent = `${revision.revision_id} \u00b7 ${revision.state_after_commit}`;
    restoreSource.append(option);
  });
  setManagementControls(capabilities.enabled);
  if (!consoleState.currentDraft) {
    document.querySelector("#save-draft").disabled = true;
    document.querySelector("#preview-draft").disabled = true;
    document.querySelector("#restore-draft").disabled = true;
    document.querySelector("#remove-note").disabled = true;
  }
  document.querySelector("#commit-draft").disabled = !consoleState.currentPreview?.applicable || !capabilities.enabled;
  syncDraftSelections();
  document.querySelector("#control-panel").hidden = false;
  document.querySelector("#eligible").hidden = false;
  document.querySelector("#editor").hidden = false;
}

export function setDraftMessage(message, isError = false) {
  const node = document.querySelector("#draft-message");
  node.textContent = message;
  node.dataset.state = isError ? "error" : "";
}

export function renderProviderSessions(capabilitiesEnvelope, listEnvelope) {
  const capabilities = capabilitiesEnvelope.value || {};
  const list = listEnvelope.value || {};
  showText("#session-badge", capabilitiesEnvelope.operation === "capabilities" ? "fixture-only" : "available");
  showText("#session-mode", "fixture_only");
  showText("#session-profile", capabilities.profile_id);
  showText("#session-evidence", capabilities.evidence);
  showText("#session-transport", capabilities.transport);
  showText("#session-hardening", capabilities.hardening
    ? `tools=${capabilities.hardening.tools_enabled ? "on" : "off"} \u00b7 ambient=${capabilities.hardening.ambient_history ? "on" : "off"}`
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
  const operationRows = document.querySelector("#session-operation-rows");
  operationRows.replaceChildren();
  (list.operations || []).forEach((operation) => {
    const row = document.createElement("tr");
    row.dataset.operationState = String(operation.state ?? "unavailable");
    [operation.operation_id, operation.kind, operation.state, operation.automatic_retry ? "retry allowed" : "no retry", operation.auto_resume ? "auto" : "held", operation.game_effects]
      .forEach((value) => { const cell = document.createElement("td"); cell.textContent = String(value ?? "unavailable"); row.append(cell); });
    operationRows.append(row);
  });
  document.querySelector("#session-panel").hidden = false;
}

export function renderDraft(draft) {
  consoleState.currentDraft = draft;
  showText("#draft-version", `Draft ${draft.draft_id} \u00b7 version ${draft.version}`);
  document.querySelector("#save-draft").disabled = !controlCapabilities?.enabled;
  document.querySelector("#preview-draft").disabled = !controlCapabilities?.enabled;
  document.querySelector("#restore-draft").disabled = !controlCapabilities?.enabled;
  document.querySelector("#remove-note").disabled = !controlCapabilities?.enabled
    || !draft.note_items.some((item) => item.item_id === "note-browser");
  syncDraftSelections();
}

export function renderPreview(preview) {
  consoleState.currentPreview = preview;
  showText("#preview-id", preview.preview_id);
  showText("#prepared-digest", preview.prepared_manifest_sha256);
  showText("#preview-components", preview.components.length ? preview.components.map((item) => `${item.kind}:${item.bytes} B`).join(", ") : "none");
  showText("#preview-budget", `${preview.budget_status}${preview.unknown_total_risk_acknowledged ? " \u00b7 risk acknowledged" : ""}`);
  showText("#preview-provider-context", preview.provider_added_context);
  showText("#preview-badge", preview.applicable ? "applicable" : "exploratory / blocked");
  const diff = {
    blockers: preview.blockers,
    selected_items: preview.selected_items,
    notes: consoleState.currentDraft?.note_items ?? [],
    objective: consoleState.currentDraft?.objective_item ?? null,
    components: preview.components,
  };
  document.querySelector("#preview-diff").textContent = JSON.stringify(diff, null, 2);
  document.querySelector("#preview").hidden = false;
  document.querySelector("#commit-draft").disabled = !preview.applicable;
}

export function invalidatePreview() {
  consoleState.currentPreview = null;
  document.querySelector("#preview").hidden = true;
  document.querySelector("#preview-id").textContent = "";
  document.querySelector("#prepared-digest").textContent = "";
  document.querySelector("#preview-diff").textContent = "";
  document.querySelector("#commit-draft").disabled = true;
}

export function itemKey(item) {
  return `${item.item_id}:${item.version}:${item.sha256}`;
}
