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
  PolicyOwnerError,
  adoptImportedProviderSessionPolicy,
  adoptProviderSessionPolicyProposal,
  approveProviderSessionPolicy,
  getProviderSessionPolicy,
  importProviderSessionPolicy,
  maxPolicyBytes,
  proposeProviderSessionPolicy,
} from "./policy-owner.js";
import {
  ContextOwnerError,
  getContextOwnerAssociation,
  getContextOwnerEffectiveLimits,
  recoverContextControlReceipt,
} from "./context-owner.js";
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
let policyOwnerSession;
let policyOwnerGeneration = 0;
let contextOwnerSession;
let contextOwnerGeneration = 0;

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

function policyOwnerNode(tagName, text, className) {
  const node = document.createElement(tagName);
  if (text !== undefined) node.textContent = text;
  if (className) node.className = className;
  return node;
}

function addPolicyOwnerFact(parent, label, value) {
  const row = policyOwnerNode("div");
  row.append(policyOwnerNode("dt", label), policyOwnerNode("dd", value));
  parent.append(row);
}

function appendPolicyOption(select, value, label, disabled = false) {
  const option = policyOwnerNode("option", label);
  option.value = value;
  option.disabled = disabled;
  select.append(option);
}

function clearPolicyOwnerDraftInputs() {
  document.querySelector("#policy-owner-import-file").value = "";
  document.querySelector("#policy-owner-target-file").value = "";
  document.querySelector("#policy-owner-proposal-id").value = "";
  document.querySelector("#policy-owner-adopt-import").value = "";
  document.querySelector("#policy-owner-source").value = "";
  document.querySelectorAll("#policy-owner-proposals [data-approval-for]").forEach((input) => {
    input.value = "";
  });
}

function invalidatePolicyOwnerDisplay(badgeText, messageText) {
  policyOwnerGeneration += 1;
  policyOwnerSession = undefined;
  document.querySelector("#policy-owner-content").hidden = true;
  document.querySelector("#policy-owner-badge").textContent = badgeText;
  const message = document.querySelector("#policy-owner-message");
  message.dataset.state = "";
  message.textContent = messageText;
  clearPolicyOwnerDraftInputs();
  document.querySelector("#policy-owner-active").replaceChildren();
  document.querySelector("#policy-owner-history").replaceChildren();
  document.querySelector("#policy-owner-proposals").replaceChildren();
  document.querySelector("#policy-owner-revision").textContent = "";
}

function renderPolicyOwner(view) {
  const active = document.querySelector("#policy-owner-active");
  active.replaceChildren();
  if (view.active) {
    addPolicyOwnerFact(active, "Identity", `${view.active.policy_id}@${view.active.version}`);
    addPolicyOwnerFact(active, "Digest", view.active.sha256);
    addPolicyOwnerFact(active, "Mode and continuity", `${view.active.mode} · ${view.active.continuity}`);
    addPolicyOwnerFact(active, "Limits", `${view.active.max_completed_turns} turns · ${view.active.history_ttl_seconds} seconds`);
  } else {
    active.append(policyOwnerNode("p", "No policy is currently adopted for this workflow run.", "muted"));
  }
  document.querySelector("#policy-owner-revision").textContent = `Workflow run ${view.run_id} · owner revision ${view.revision}`;

  const history = document.querySelector("#policy-owner-history");
  const adoptChoices = document.querySelector("#policy-owner-adopt-import");
  const sourceChoices = document.querySelector("#policy-owner-source");
  history.replaceChildren();
  adoptChoices.replaceChildren();
  sourceChoices.replaceChildren();
  appendPolicyOption(adoptChoices, "", "Select an imported policy…");
  appendPolicyOption(sourceChoices, "", "Select a source policy…");
  for (const policy of view.history) {
    const summary = `${policy.policy_id}@${policy.version} · ${policy.mode} · ${policy.continuity}`;
    const row = policyOwnerNode("li");
    row.append(policyOwnerNode("code", policy.sha256), document.createTextNode(` · ${summary}${policy.active ? " · active" : ""}`));
    history.append(row);
    appendPolicyOption(
      adoptChoices,
      policy.sha256,
      `${summary} · ${policy.sha256.slice(0, 12)}…`,
      policy.active,
    );
    appendPolicyOption(sourceChoices, policy.sha256, `${summary} · ${policy.sha256.slice(0, 12)}…`);
  }
  if (view.history.length === 0) history.append(policyOwnerNode("li", "No policy bytes have been imported."));

  const proposals = document.querySelector("#policy-owner-proposals");
  proposals.replaceChildren();
  if (view.proposals.length === 0) {
    proposals.append(policyOwnerNode("li", "No migration proposals have been recorded."));
  }
  for (const proposal of view.proposals) {
    const row = policyOwnerNode("li");
    row.append(policyOwnerNode("strong", `${proposal.proposal_id} · ${proposal.state}`));
    row.append(policyOwnerNode("p", `Proposal ${proposal.proposal_sha256}`));
    row.append(policyOwnerNode("p", `Source ${proposal.source_sha256} → target ${proposal.target_sha256}`));
    row.append(policyOwnerNode(
      "p",
      `${proposal.approval_recorded ? "Approval recorded by owner." : "No approval recorded."}${proposal.adopted_policy_sha256 ? ` Adopted ${proposal.adopted_policy_sha256}.` : ""}`,
    ));
    if (proposal.state !== "adopted") {
      const label = policyOwnerNode("label", `Approval reference for ${proposal.proposal_id}`);
      const approval = document.createElement("input");
      approval.type = "password";
      approval.autocomplete = "off";
      approval.maxLength = 128;
      approval.dataset.approvalFor = proposal.proposal_id;
      label.append(approval);
      row.append(label);
      const action = policyOwnerNode(
        "button",
        proposal.state === "proposed" ? "Record approval" : "Adopt approved proposal",
      );
      action.type = "button";
      action.disabled = true;
      approval.addEventListener("input", () => {
        action.disabled = !/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/.test(approval.value);
      });
      action.addEventListener("click", () => {
        const token = policyOwnerSession?.token;
        const runId = policyOwnerSession?.runId;
        if (!token || !runId || action.disabled) return;
        const mutate = proposal.state === "proposed"
          ? () => approveProviderSessionPolicy(runId, token, proposal.proposal_id, proposal.proposal_sha256, approval.value, policyOwnerSession.view.revision)
          : () => adoptProviderSessionPolicyProposal(runId, token, proposal.proposal_id, proposal.proposal_sha256, approval.value, policyOwnerSession.view.revision);
        void performPolicyOwnerCommand(
          proposal.state === "proposed" ? "Proposal approval" : "Proposal adoption",
          mutate,
        );
      });
      row.append(action);
    }
    proposals.append(row);
  }
}

async function refreshPolicyOwner(notice = "Current owner history loaded.") {
  const generation = ++policyOwnerGeneration;
  const runId = document.querySelector("#policy-owner-run-id").value;
  const token = document.querySelector("#policy-owner-token").value;
  const content = document.querySelector("#policy-owner-content");
  const message = document.querySelector("#policy-owner-message");
  const badge = document.querySelector("#policy-owner-badge");
  content.hidden = true;
  clearPolicyOwnerDraftInputs();
  document.querySelector("#policy-owner-active").replaceChildren();
  document.querySelector("#policy-owner-history").replaceChildren();
  document.querySelector("#policy-owner-proposals").replaceChildren();
  policyOwnerSession = undefined;
  message.dataset.state = "";
  message.textContent = "Loading current policy-owner metadata…";
  try {
    const response = await getProviderSessionPolicy(runId, token);
    if (generation !== policyOwnerGeneration) return;
    policyOwnerSession = { runId, token, view: response.value };
    renderPolicyOwner(response.value);
    content.hidden = false;
    badge.textContent = "authenticated owner";
    message.textContent = notice;
  } catch (error) {
    if (generation !== policyOwnerGeneration) return;
    badge.textContent = "owner unavailable";
    message.dataset.state = "error";
    if (error instanceof PolicyOwnerError && error.status === 403) {
      message.textContent = "Access denied. The owner token needs the scoped workflow grant for this request.";
    } else if (error instanceof PolicyOwnerError && error.status === 409) {
      message.textContent = "Owner state changed or the run does not match. Refresh and verify the workflow run ID.";
    } else if (error instanceof PolicyOwnerError && error.status === 404) {
      message.textContent = "The workflow run or saved-policy owner is unavailable.";
    } else {
      message.textContent = error instanceof PolicyOwnerError && error.status === 400
        ? error.message
        : "Could not load the authenticated saved-policy owner. No local policy history is substituted.";
    }
  }
}

async function performPolicyOwnerCommand(label, action) {
  const generation = policyOwnerGeneration;
  const message = document.querySelector("#policy-owner-message");
  message.dataset.state = "";
  message.textContent = `${label} is being submitted with the current owner revision…`;
  try {
    const result = await action();
    if (generation !== policyOwnerGeneration) return;
    await refreshPolicyOwner(`${label} recorded at owner revision ${result.revision}. Approval references were cleared; re-enter one before a later approval or adoption.`);
  } catch (error) {
    if (generation !== policyOwnerGeneration) return;
    message.dataset.state = "error";
    if (error instanceof PolicyOwnerError && error.status === 403) {
      message.textContent = "Access denied. Check workflow:control and, for uploaded policy files, workflow:content:write.";
    } else if (error instanceof PolicyOwnerError && error.status === 409) {
      message.textContent = "The owner rejected a stale or conflicting change. Reload owner history before trying again.";
    } else if (error instanceof PolicyOwnerError && error.status === 400) {
      message.textContent = error.message;
    } else {
      message.textContent = `${label} could not be confirmed. Refresh owner history before retrying.`;
    }
  }
}

async function submitPolicyOwnerImport(event) {
  event.preventDefault();
  if (!policyOwnerSession) return;
  const file = document.querySelector("#policy-owner-import-file").files?.[0];
  if (!file || file.size === 0 || file.size > maxPolicyBytes
    || !(file.type === "application/json" || file.name.toLowerCase().endsWith(".json"))) {
    document.querySelector("#policy-owner-message").textContent = `Choose a non-empty JSON file no larger than ${maxPolicyBytes} bytes.`;
    return;
  }
  const { runId, token, view } = policyOwnerSession;
  await performPolicyOwnerCommand("Policy import", async () =>
    importProviderSessionPolicy(runId, token, view.revision, await file.arrayBuffer()));
}

async function submitPolicyOwnerProposal(event) {
  event.preventDefault();
  if (!policyOwnerSession) return;
  const file = document.querySelector("#policy-owner-target-file").files?.[0];
  if (!file || file.size === 0 || file.size > maxPolicyBytes
    || !(file.type === "application/json" || file.name.toLowerCase().endsWith(".json"))) {
    document.querySelector("#policy-owner-message").textContent = `Choose a non-empty JSON file no larger than ${maxPolicyBytes} bytes.`;
    return;
  }
  const sourceSha256 = document.querySelector("#policy-owner-source").value;
  const proposalId = document.querySelector("#policy-owner-proposal-id").value;
  const { runId, token, view } = policyOwnerSession;
  await performPolicyOwnerCommand("Migration proposal", async () =>
    proposeProviderSessionPolicy(runId, token, proposalId, sourceSha256, view.revision, await file.arrayBuffer()));
}

function wirePolicyOwner() {
  document.querySelector("#policy-owner-connect-form").addEventListener("submit", (event) => {
    event.preventDefault();
    void refreshPolicyOwner();
  });
  for (const selector of ["#policy-owner-run-id", "#policy-owner-token"]) {
    document.querySelector(selector).addEventListener("input", () => {
      invalidatePolicyOwnerDisplay(
        "owner selection changed",
        "Workflow run or owner token changed. Refresh to load the scoped policy history.",
      );
    });
  }
  document.querySelector("#policy-owner-clear").addEventListener("click", () => {
    document.querySelector("#policy-owner-token").value = "";
    document.querySelector("#policy-owner-run-id").value = "";
    invalidatePolicyOwnerDisplay(
      "credentials cleared",
      "Owner credentials and visible policy selections were cleared from this tab.",
    );
  });
  document.querySelector("#policy-owner-import-form").addEventListener("submit", (event) => {
    void submitPolicyOwnerImport(event);
  });
  document.querySelector("#policy-owner-propose-form").addEventListener("submit", (event) => {
    void submitPolicyOwnerProposal(event);
  });
  document.querySelector("#policy-owner-adopt-import-button").addEventListener("click", () => {
    if (!policyOwnerSession) return;
    const policySha256 = document.querySelector("#policy-owner-adopt-import").value;
    const { runId, token, view } = policyOwnerSession;
    void performPolicyOwnerCommand("Initial policy adoption", () =>
      adoptImportedProviderSessionPolicy(runId, token, policySha256, view.revision));
  });
  document.querySelector("#policy-owner-adopt-import").addEventListener("change", (event) => {
    document.querySelector("#policy-owner-adopt-import-button").disabled = event.target.value.length === 0;
  });
}

function clearContextOwnerDisplay(badgeText, messageText) {
  contextOwnerGeneration += 1;
  contextOwnerSession = undefined;
  document.querySelector("#context-owner-content").hidden = true;
  document.querySelector("#context-owner-badge").textContent = badgeText;
  const message = document.querySelector("#context-owner-message");
  message.dataset.state = "";
  message.textContent = messageText;
  document.querySelector("#context-owner-association").replaceChildren();
  document.querySelector("#context-owner-limits").replaceChildren();
  document.querySelector("#context-owner-revision").textContent = "";
  document.querySelector("#context-owner-receipt").textContent = "No historical receipt recovered.";
}

function addContextFact(parent, label, value) {
  const row = document.createElement("div");
  const term = document.createElement("dt");
  const detail = document.createElement("dd");
  term.textContent = label;
  detail.textContent = value;
  row.append(term, detail);
  parent.append(row);
}

function renderContextOwner(association, limits) {
  const binding = association.binding;
  const associationNode = document.querySelector("#context-owner-association");
  associationNode.replaceChildren();
  for (const [label, value] of [
    ["Owner", `${binding.owner_id}@${binding.owner_version}`],
    ["Invocation", binding.invocation_id],
    ["Binding", `${binding.binding_id}@${binding.binding_version}`],
    ["Context", `${binding.context_ref} · ${binding.node_kind}`],
    ["Cursor", `${binding.graph_id}/${binding.node_id}/${binding.node_execution_id}`],
    ["Binding state", `${binding.state} · lease ${binding.lease_epoch} · plan ${binding.plan_epoch}`],
    ["Grants", Object.entries(binding.grants).filter(([, enabled]) => enabled).map(([name]) => name).join(", ") || "none"],
    ["Continuity", binding.continuity.survives_controller_restart ? "survives controller restart" : "restart continuity unavailable"],
  ]) addContextFact(associationNode, label, value);

  const limitsNode = document.querySelector("#context-owner-limits");
  limitsNode.replaceChildren();
  for (const [label, value] of [
    ["Owner", `${limits.owner_id}@${limits.owner_version}`],
    ["Catalog digest", limits.catalog_digest],
    ["Adapter/model", `${limits.adapter_revision} · ${limits.model_revision}`],
    ["Items", limits.effective_limits.max_items],
    ["Notes", limits.effective_limits.max_notes],
    ["Context bytes", limits.effective_limits.max_context_bytes],
    ["Objective bytes", limits.effective_limits.max_objective_bytes],
    ["Control events", limits.effective_limits.max_control_events],
  ]) addContextFact(limitsNode, label, String(value));
}

async function refreshContextOwner(notice = "Current owner association and effective limits loaded.") {
  const runId = document.querySelector("#context-owner-run-id").value;
  const token = document.querySelector("#context-owner-token").value;
  clearContextOwnerDisplay("loading owner", "Clearing stale owner data and reading the current Harness association…");
  const generation = contextOwnerGeneration;
  try {
    const [association, limits] = await Promise.all([
      getContextOwnerAssociation(runId, token),
      getContextOwnerEffectiveLimits(runId, token),
    ]);
    if (generation !== contextOwnerGeneration) return;
    if (association.binding.binding_id !== limits.binding_id
      || association.binding.binding_digest !== limits.binding_digest
      || association.binding.owner_id !== limits.owner_id
      || association.binding.owner_version !== limits.owner_version
      || association.binding.binding_version !== limits.binding_version
      || association.binding.context_ref !== limits.context_ref
      || association.binding.node_kind !== limits.node_kind
      || association.binding.boundary.adapter_revision !== limits.adapter_revision
      || association.binding.boundary.model_revision !== limits.model_revision) {
      throw new ContextOwnerError("Owner association and effective limits identify different bindings", 409, "context_owner_identity_mismatch");
    }
    contextOwnerSession = { runId, token, association, limits };
    renderContextOwner(association, limits);
    document.querySelector("#context-owner-content").hidden = false;
    document.querySelector("#context-owner-badge").textContent = "authenticated current owner";
    document.querySelector("#context-owner-message").textContent = notice;
  } catch (error) {
    if (generation !== contextOwnerGeneration) return;
    const message = document.querySelector("#context-owner-message");
    message.dataset.state = "error";
    document.querySelector("#context-owner-badge").textContent = "owner unavailable";
    message.textContent = error instanceof ContextOwnerError && error.status === 403
      ? "Access denied. The owner token needs the scoped workflow read grant."
      : error instanceof ContextOwnerError && error.status === 409
        ? "The current owner identity changed or is unavailable. Refresh the selected run."
        : error instanceof ContextOwnerError && error.status === 400
          ? error.message
          : "Could not load the current owner. No historical grant or local fixture was substituted.";
  }
}

async function recoverContextOwnerReceipt() {
  if (!contextOwnerSession) return;
  const output = document.querySelector("#context-owner-receipt");
  try {
    const command = JSON.parse(document.querySelector("#context-owner-recovery-command").value);
    const receipt = await recoverContextControlReceipt(
      contextOwnerSession.runId,
      contextOwnerSession.token,
      command,
    );
    output.textContent = JSON.stringify(receipt, null, 2);
  } catch (error) {
    output.textContent = error instanceof ContextOwnerError ? error.message : "Receipt recovery failed.";
  }
}

function wireContextOwner() {
  document.querySelector("#context-owner-connect-form").addEventListener("submit", (event) => {
    event.preventDefault();
    void refreshContextOwner();
  });
  for (const selector of ["#context-owner-run-id", "#context-owner-token"]) {
    document.querySelector(selector).addEventListener("input", () => {
      clearContextOwnerDisplay(
        "owner selection changed",
        "Workflow run or owner token changed. Refresh to read the new current association.",
      );
    });
  }
  document.querySelector("#context-owner-clear").addEventListener("click", () => {
    document.querySelector("#context-owner-run-id").value = "";
    document.querySelector("#context-owner-token").value = "";
    clearContextOwnerDisplay("credentials cleared", "Owner credentials and current association data were cleared from this tab.");
  });
  document.querySelector("#context-owner-recover").addEventListener("click", () => {
    void recoverContextOwnerReceipt();
  });
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

wirePolicyOwner();
wireContextOwner();
loadFixture().catch((error) => {
  showError(error instanceof Error ? error.message : "bundle could not be read");
});
