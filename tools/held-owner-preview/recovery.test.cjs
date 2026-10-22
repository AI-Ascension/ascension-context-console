// SPDX-License-Identifier: MIT
const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { startProductionPolicyStack } = require("../browser-audit/production-policy-stack.cjs");
const { recordingForwarder, successful, request } = require("../owner-process-recovery/http.cjs");
const { createOperator } = require("../owner-process-recovery/operator.cjs");
const { prepareOwner } = require("../owner-process-recovery/owner.cjs");
const sourceLock = require("../../contracts/held-owner-preview.lock.json");

async function fixture(t) {
  const parent = process.env.CONSOLE_OWNER_PRIVATE_TEST_ROOT;
  assert.ok(parent && path.isAbsolute(parent), "CONSOLE_OWNER_PRIVATE_TEST_ROOT must name a private directory with trusted ancestors");
  assert.equal(fs.statSync(parent).mode & 0o077, 0, "private fixture parent permissions");
  const stack = await startProductionPolicyStack(sourceLock);
  t.after(() => stack.close());
  const owner = await prepareOwner(stack);
  const forwarder = await recordingForwarder(stack.ownerAddress);
  t.after(() => forwarder.close());
  const operator = await createOperator({ parent, harnessAddress: forwarder.address, runId: owner.runId, harnessBearer: stack.ownerHeaders.authorization.slice(7) });
  t.after(() => operator.close());
  return { stack, owner: { ...owner, prepareHeldPreview: () => prepareHeldPreview(stack, owner) }, forwarder, operator };
}

async function prepareHeldPreview(stack, owner) {
  const current = successful(await request(stack.ownerAddress, owner.runPath, { headers: stack.ownerHeaders }), "current workflow before held render");
  return request(stack.ownerAddress, owner.runPath + "/commands", { method: "POST", headers: stack.ownerHeaders, body: {
    schema_version: "ascension.management/v1", command_id: "console-process-held-render", run_id: owner.runId,
    expected_revision: current.run.run_revision, actor_scope: "profile:console-live", kind: "step", parameters: {},
  } });
}

test("held edits preview and commit through Console, then exact control recovery never repeats effects", { timeout: 120_000 }, async (t) => {
  const { stack, owner, forwarder, operator } = await fixture(t);
  await operator.start();
  const effectsBefore = await owner.effects();
  assert.deepEqual(effectsBefore, { mod: 0, provider: 0 }, "fixture setup starts with zero effects");
  const ok = async (body, label, suffix) => {
    const response = await operator.send(body, suffix);
    if (response.status !== 200) t.diagnostic(JSON.stringify(forwarder.calls.at(-1)));
    return successful(response, label);
  };
  const invoke = (operation, binding, grant) => operator.invocation(operation, binding, grant);
  const association = async () => (await ok(invoke({ operation: "current_association" }), "current association")).value.binding;
  let binding = await association();
  await ok(invoke({ operation: "submit_control", command: { pause: {
    idempotency_key: "held-pause", expected_control_version: binding.boundary.control_version,
  } } }, binding, "pause"), "hold context");
  const heldRender = await owner.prepareHeldPreview();
  assert.equal(heldRender.status, 200, "the owner prepares the trusted held render successfully");
  t.diagnostic(JSON.stringify({ heldRenderStatus: heldRender.status, code: heldRender.value?.error?.code ?? null }));
  assert.deepEqual(await owner.effects(), effectsBefore, "the held host-thread preparation cannot write to the provider");
  binding = await association();
  const heldBinding = structuredClone(binding);
  const draftId = "held-draft";
  const created = await ok(invoke({ operation: "create_draft", request: {
    schema_version: "ascension.harness.context-owner-draft-request.v1", request_id: "held-create", draft_id: draftId,
    base_revision_id: binding.approved_revision_id, expected_boundary: binding.boundary,
  } }, binding, "edit"), "create held draft");
  let draft = created.value.result.value.draft;
  const metadata = invoke({ operation: "eligible_items", query: { draft_id: draftId, include_content: false } }, binding);
  const items = (await ok(metadata, "metadata eligible items")).value.items;
  assert.ok(items.length > 0);
  assert.ok(items.every((item) => !Object.hasOwn(item, "content")));
  const content = structuredClone(metadata);
  content.operation.query.include_content = true;
  const beforeDeniedContent = forwarder.calls.length;
  assert.equal((await operator.send(content)).status, 403);
  assert.equal(forwarder.calls.length, beforeDeniedContent);
  await operator.stop();
  operator.provisionGrant("content", "context.content.read");
  operator.provisionGrant("objective", "context.objective.edit");
  await operator.start();
  const retained = (await ok(content, "separately authorized content")).value.items;
  assert.equal(retained[0].content.length, retained[0].byte_length);
  const note = `held-private-note-${crypto.randomBytes(24).toString("hex")}`;
  const edited = await ok(invoke({ operation: "patch_draft", draft_id: draftId, request: {
    schema_version: "ascension.harness.context-owner-draft-patch.v1", request_id: "held-edit", draft_id: draftId,
    expected_version: draft.version, expected_boundary: binding.boundary,
    operations: [
      { operation: "exclude_item", reference: items[0].reference }, { operation: "include_item", reference: items[0].reference },
      { operation: "pin_item", item_id: items[0].reference.item_id }, { operation: "unpin_item", item_id: items[0].reference.item_id },
      { operation: "pin_item", item_id: items[0].reference.item_id }, { operation: "put_note", note_id: "held-note", text: note },
      { operation: "set_objective", text: "A separately authorized local fixture objective" },
    ],
  } }, binding, "edit"), "authorized held edit and objective");
  draft = edited.value.result.value.draft;
  assert.ok(draft.pinned_item_ids.includes(items[0].reference.item_id));
  assert.equal(draft.notes[0].attributed_to, "profile:console-live");
  assert.ok(draft.objective);
  const previewBody = invoke({ operation: "create_preview", draft_id: draftId, request: {
    schema_version: "ascension.harness.context-owner-preview-request.v1", request_id: "held-preview", draft_id: draftId,
    expected_version: draft.version, expected_boundary: binding.boundary,
  } }, binding, "edit");
  const preview = (await ok(previewBody, "production preview")).value.result.value;
  assert.deepEqual(preview.blockers, []);
  const readPreview = await ok(invoke({ operation: "get_preview", preview_id: preview.preview_id }), "retained preview");
  assert.deepEqual(readPreview.value, preview);
  const repeatedPreview = structuredClone(previewBody);
  repeatedPreview.operation.request.request_id = "held-preview-again";
  assert.equal((await ok(repeatedPreview, "deterministic preview")).value.result.value.manifest_digest, preview.manifest_digest);
  assert.deepEqual(await owner.effects(), effectsBefore, "edits and previews do not invoke the provider or Mod effects");
  const committed = await ok(invoke({ operation: "submit_control", command: { commit: {
    idempotency_key: "held-commit", expected_control_version: binding.boundary.control_version,
    expected_revision_id: binding.approved_revision_id, expected_boundary: binding.boundary,
    preview_manifest_digest: preview.manifest_digest, approved_manifest_digest: preview.manifest_digest,
  } } }, binding, "commit"), "commit while held");
  assert.equal(committed.value.effect, "revision_committed");
  assert.notEqual(committed.value.revision_id, binding.approved_revision_id);
  binding = await association();
  const resumed = await ok(invoke({ operation: "submit_control", command: { resume: {
    idempotency_key: "held-resume", expected_control_version: binding.boundary.control_version, expected_boundary: binding.boundary,
  } } }, binding, "resume"), "explicitly resume");
  assert.equal(resumed.value.effect, "resume_accepted");
  binding = await association();
  const stale = invoke({ operation: "create_draft", request: {
    schema_version: "ascension.harness.context-owner-draft-request.v1", request_id: "stale-held-create", draft_id: "stale-held-draft",
    base_revision_id: heldBinding.approved_revision_id, expected_boundary: heldBinding.boundary,
  } }, heldBinding, "edit");
  assert.equal((await operator.send(stale)).status, 409, "an old boundary cannot authorize a new owner write");
  const finalPause = invoke({ operation: "submit_control", command: { pause: {
    idempotency_key: "lost-control-pause", expected_control_version: binding.boundary.control_version,
  } } }, binding, "pause");
  forwarder.loseNext("POST", `${owner.runPath}/context-control-commands`);
  assert.equal((await operator.send(finalPause)).status, 409);
  const lost = forwarder.lostResponse();
  assert.equal(lost.status, 200);
  assert.equal(lost.value.effect, "pause_requested");
  const count = () => forwarder.calls.filter((call) => call.method === "POST" && call.path === lost.path && call.digest === lost.digest).length;
  assert.equal(count(), 1);
  await operator.stop();
  await stack.restartOwner();
  await operator.start();
  const recovery = structuredClone(finalPause);
  recovery.identity.console = invoke({}, undefined, "read").identity.console;
  const recovered = await ok(recovery, "exact historical control receipt", "/receipt-lookup");
  assert.equal(recovered.kind, "control_receipt");
  assert.deepEqual(recovered.value, lost.value);
  assert.deepEqual(await ok(recovery, "cached control receipt", "/cached-result"), recovered);
  assert.equal(count(), 1);
  const effectsAfter = await owner.effects();
  assert.deepEqual(effectsAfter, effectsBefore, "control/recovery do not execute a workflow step");
  t.diagnostic(JSON.stringify({ effectsBefore, effectsAfter, recoveredControlWriteCount: count() }));
  operator.assertPrivate(note);
});
