// SPDX-License-Identifier: MIT
const assert = require("node:assert/strict");
const test = require("node:test");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const { startProductionPolicyStack } = require("../browser-audit/production-policy-stack.cjs");
const { recordingForwarder, successful } = require("./http.cjs");
const { createOperator } = require("./operator.cjs");
const { prepareOwner } = require("./owner.cjs");
const sourceLock = require("../../contracts/owner-process-recovery.lock.json");

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
  return { stack, owner, forwarder, operator };
}

test("real Console ingress recovers exact owner effects across independent process restarts", { timeout: 120_000 }, async (t) => {
  const { stack, owner, forwarder, operator } = await fixture(t);
  const firstPid = await operator.start();
  const effectsBefore = await owner.effects();
  const ok = async (invocation, label, suffix) => successful(await operator.send(invocation, suffix), label);
  const read = (operation, binding) => operator.invocation(operation, binding);
  const association = async () => (await ok(read({ operation: "current_association" }), "association")).value.binding;
  let binding = await association();
  assert.equal(binding.owner_id, "console-served-context-owner");
  assert.equal((await ok(read({ operation: "current_effective_limits" }, binding), "limits")).kind, "effective_limits");

  await t.test("untrusted identity and metadata grants cannot dispatch writes", async () => {
    const before = forwarder.calls.length;
    const ordinary = read({ operation: "current_association" });
    for (const [headers, status] of [
      [{ host: "foreign.example" }, 400], [{ origin: "https://foreign.example" }, 400],
      [{ authorization: "Bearer forged" }, 401], [{ "x-principal": "profile:console-live" }, 400],
    ]) assert.equal((await operator.send(ordinary, "", headers)).status, status);
    const foreign = structuredClone(ordinary);
    foreign.identity.console_scope.episode_id = "foreign-episode";
    assert.equal((await operator.send(foreign)).status, 403);
    const create = operator.invocation({ operation: "create_draft", request: {
      schema_version: "ascension.harness.context-owner-draft-request.v1", request_id: "deny-metadata", draft_id: "deny-metadata",
      base_revision_id: binding.approved_revision_id, expected_boundary: binding.boundary,
    } }, binding);
    assert.equal((await operator.send(create)).status, 403);
    create.identity.console = operator.invocation({}, undefined, "edit").identity.console;
    assert.equal((await operator.send(create, "", { "x-csrf-token": "forged" })).status, 401);
    assert.equal(forwarder.calls.length, before, "all ingress denials precede owner dispatch");
  });

  const pause = operator.invocation({ operation: "submit_control", command: { pause: {
    idempotency_key: "console-process-pause", expected_control_version: binding.boundary.control_version,
  } } }, binding, "pause");
  assert.equal((await ok(pause, "pause")).value.effect, "pause_requested");
  binding = await association();
  const draftId = "console-process-draft";
  const create = operator.invocation({ operation: "create_draft", request: {
    schema_version: "ascension.harness.context-owner-draft-request.v1", request_id: "console-process-create", draft_id: draftId,
    base_revision_id: binding.approved_revision_id, expected_boundary: binding.boundary,
  } }, binding, "edit");
  const created = await ok(create, "create draft");
  assert.equal(created.kind, "mutation_receipt");
  let draft = created.value.result.value.draft;
  assert.equal(draft.draft_id, draftId);
  const listed = await ok(read({ operation: "draft_list" }), "draft list");
  assert.equal(listed.value.drafts.length, 1);
  const revisions = await ok(read({ operation: "revision_page", query: { after_revision_id: null, limit: 10 } }), "revision page");
  assert.equal(revisions.kind, "revision_page");
  assert.equal(revisions.value.schema_version, "ascension.harness.context-owner-revision.v1");
  assert.deepEqual((await ok(read({ operation: "get_draft", draft_id: draftId }), "get draft")).value.draft, draft);
  const items = (await ok(read({ operation: "eligible_items", query: { draft_id: draftId, include_content: false } }, binding), "items")).value.items;
  assert.ok(items.length > 0);
  assert.ok(items.every((item) => !Object.hasOwn(item, "content")));
  const sentinel = `private-process-note-${crypto.randomBytes(24).toString("hex")}`;
  const patch = operator.invocation({ operation: "patch_draft", draft_id: draftId, request: {
    schema_version: "ascension.harness.context-owner-draft-patch.v1", request_id: "console-process-patch", draft_id: draftId,
    expected_version: draft.version, expected_boundary: binding.boundary,
    operations: [{ operation: "pin_item", item_id: items[0].reference.item_id }, { operation: "put_note", note_id: "process-note", text: sentinel }],
  } }, binding, "edit");
  await t.test("objective and resume require their separate grants", async () => {
    const before = forwarder.calls.length;
    const objective = structuredClone(patch);
    objective.operation.request.operations = [{ operation: "set_objective", text: "denied objective" }];
    assert.equal((await operator.send(objective)).status, 403);
    const resume = operator.invocation({ operation: "submit_control", command: { resume: {
      idempotency_key: "denied-resume", expected_control_version: binding.boundary.control_version, expected_boundary: binding.boundary,
    } } }, binding, "edit");
    assert.equal((await operator.send(resume)).status, 403);
    assert.equal(forwarder.calls.length, before);
  });

  forwarder.loseNext("PATCH", `${owner.runPath}/context-owner-drafts/${draftId}`);
  const ambiguous = await operator.send(patch);
  assert.equal(ambiguous.status, 409);
  assert.equal(ambiguous.value.error, "exact_recovery_required");
  const lost = forwarder.lostResponse();
  assert.equal(lost.status, 200, "actual owner committed before its reply was dropped");
  assert.equal(lost.value.request_id, patch.operation.request.request_id);
  draft = lost.value.result.value.draft;
  assert.equal(draft.version, patch.operation.request.expected_version + 1);
  assert.ok(draft.pinned_item_ids.includes(items[0].reference.item_id));
  assert.equal(draft.notes[0].reference.sha256, crypto.createHash("sha256").update(sentinel).digest("hex"));
  assert.equal(draft.notes[0].attributed_to, "profile:console-live");
  const mutationCount = () => forwarder.calls.filter((call) => call.method === "PATCH" && call.path === lost.path).length;
  assert.equal(mutationCount(), 1);

  await operator.stop();
  await stack.restartOwner();
  const secondPid = await operator.start();
  assert.notEqual(firstPid, secondPid);
  const recovery = structuredClone(patch);
  recovery.identity.console = operator.invocation({}, undefined, "read").identity.console;
  const recovered = await ok(recovery, "exact lookup after both process restarts", "/receipt-lookup");
  assert.equal(recovered.kind, "mutation_receipt");
  assert.deepEqual(recovered.value, lost.value);
  const cached = await ok(recovery, "cached original result", "/cached-result");
  assert.deepEqual(cached, recovered);
  assert.equal(mutationCount(), 1, "lookup and cached recovery must never repeat the PATCH");
  assert.equal((await operator.send(patch)).status, 403, "completed journal entries admit cached reads only, never an ordinary resend");
  assert.equal(mutationCount(), 1);
  const conflict = structuredClone(recovery);
  conflict.operation.request.operations[1].text = "changed under the same stable key";
  assert.equal((await operator.send(conflict, "/receipt-lookup")).status, 409);
  assert.equal(mutationCount(), 1);
  assert.deepEqual(await owner.effects(), effectsBefore, "draft/edit/recovery caused no Mod or provider effect");

  await t.test("cached result is still subject to current principal and grant authority", async () => {
    const before = forwarder.calls.length;
    await operator.stop();
    operator.revokeGrant("read");
    await operator.start();
    assert.equal((await operator.send(recovery, "/cached-result")).status, 403);
    assert.equal(forwarder.calls.length, before);
    await operator.stop();
    operator.provisionGrant("read-renewed", "context.metadata.read");
    await operator.start();
    recovery.identity.console = operator.invocation({}, undefined, "read-renewed").identity.console;
    assert.deepEqual(await ok(recovery, "cache after independent new metadata grant", "/cached-result"), recovered);
    operator.revokePrincipal();
    assert.equal((await operator.send(recovery, "/cached-result")).status, 401);
    assert.equal(forwarder.calls.length, before);
    operator.assertPrivate(sentinel);
  });
});

// This independent control receipt case does not claim the held preview/commit
// acceptance under #18; that sequence has a separately preserved owner failure.
test("a lost pause receipt survives both process restarts without repeating control effects", { timeout: 120_000 }, async (t) => {
  const { stack, owner, forwarder, operator } = await fixture(t);
  await operator.start();
  const effectsBefore = await owner.effects();
  const ok = async (body, label, suffix) => successful(await operator.send(body, suffix), label);
  const association = async () => (await ok(operator.invocation({ operation: "current_association" }), "current association")).value.binding;
  let binding = await association();
  const pause = operator.invocation({ operation: "submit_control", command: { pause: {
    idempotency_key: "lost-control-pause", expected_control_version: binding.boundary.control_version,
  } } }, binding, "pause");
  forwarder.loseNext("POST", owner.runPath + "/context-control-commands");
  const ambiguous = await operator.send(pause);
  assert.equal(ambiguous.status, 409);
  assert.equal(ambiguous.value.error, "exact_recovery_required");
  const lost = forwarder.lostResponse();
  assert.equal(lost.status, 200);
  assert.equal(lost.value.effect, "pause_requested");
  const count = () => forwarder.calls.filter((call) => call.method === "POST" && call.path === lost.path && call.digest === lost.digest).length;
  assert.equal(count(), 1);

  await operator.stop();
  await stack.restartOwner();
  await operator.start();
  const recovery = structuredClone(pause);
  recovery.identity.console = operator.invocation({}, undefined, "read").identity.console;
  const recovered = await ok(recovery, "exact historical control receipt", "/receipt-lookup");
  assert.equal(recovered.kind, "control_receipt");
  assert.deepEqual(recovered.value, lost.value);
  assert.deepEqual(await ok(recovery, "cached control receipt", "/cached-result"), recovered);
  assert.equal(count(), 1, "receipt lookup and cache never repeat the original control write");
  assert.equal((await operator.send(pause)).status, 403, "completed control entries admit cached reads only");
  assert.equal(count(), 1);
  assert.deepEqual(await owner.effects(), effectsBefore, "pause and receipt recovery never execute a workflow step");
  operator.assertPrivate("no-private-content-in-control-case");
});
