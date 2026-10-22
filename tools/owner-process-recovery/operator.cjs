// SPDX-License-Identifier: MIT
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const net = require("node:net");
const crypto = require("node:crypto");
const { execFileSync, spawn } = require("node:child_process");
const { once } = require("node:events");
const { listen, close, request } = require("./http.cjs");

const ownerId = "console-served-context-owner";
const issuer = "https://identity.example/console-process-witness";
const subject = "profile:console-live";
const audience = "https://console.example/owner";
const permissions = { read: "context.metadata.read", edit: "context.edit", pause: "context.pause", commit: "context.commit", resume: "context.resume" };

function u64(value) { const bytes = Buffer.alloc(8); bytes.writeBigUInt64BE(BigInt(value)); return bytes; }

async function createOperator({ parent, harnessAddress, runId, harnessBearer }) {
  assert.ok(path.isAbsolute(parent), "operator fixture parent must be an explicit absolute private path");
  const root = fs.mkdtempSync(path.join(parent, "owner-process-"));
  fs.chmodSync(root, 0o700);
  try {
    return await provisionOperator({ root, harnessAddress, runId, harnessBearer });
  } catch (error) {
    fs.rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

async function provisionOperator({ root, harnessAddress, runId, harnessBearer }) {
  const portReservation = net.createServer();
  const address = await listen(portReservation);
  await close(portReservation);
  const binary = process.env.CONTEXT_CONSOLE_BIN;
  assert.ok(binary && fs.statSync(binary).isFile(), "CONTEXT_CONSOLE_BIN must be built before the witness");
  const expires = Math.floor(Date.now() / 1000) + 3600;
  const scope = { project_id: "served-policy-project", run_id: runId, episode_id: "episode-served-policy-gate", agent_id: "served-policy-agent" };
  const principalSecret = crypto.randomBytes(32);
  const macKey = crypto.randomBytes(32);
  const csrf = crypto.randomBytes(32).toString("hex");
  const principal = { reference: "principal-process", issuer, subject, audience, credential_id: "credential-process", expires_at: expires, revoked: false };
  const token = `ccp1.${principal.reference}.${principalSecret.toString("hex")}`;
  let child;
  let stdout = "";
  let stderr = "";
  const history = [];
  const env = { PATH: process.env.PATH, ...(process.env.LD_LIBRARY_PATH ? { LD_LIBRARY_PATH: process.env.LD_LIBRARY_PATH } : {}) };
  const write = (name, value) => fs.writeFileSync(path.join(root, name), Buffer.isBuffer(value) ? value : JSON.stringify(value), { mode: 0o600 });
  function writePrincipal() {
    const mac = crypto.createHmac("sha256", macKey).update("ascension.console18.operator-principal.v1\0");
    for (const value of [principal.reference, issuer, subject, audience, principal.credential_id, u64(expires), Buffer.from([Number(principal.revoked)]), principalSecret]) {
      const bytes = Buffer.isBuffer(value) ? value : Buffer.from(value);
      mac.update(u64(bytes.length)).update(bytes);
    }
    write("principals.json", { schema_version: "ascension.context-console.principals.v1", records: [{ ...principal, tag: mac.digest("hex") }] });
  }
  writePrincipal();
  write("principal-mac.bin", macKey);
  write("csrf.bin", Buffer.from(csrf));
  write("owner-bearer.bin", Buffer.from(harnessBearer));
  write("index-key.bin", crypto.randomBytes(32));
  write("data-key.bin", crypto.randomBytes(32));
  write("journal-keys.json", { schema_version: "ascension.context-console.owner-journal-keys.v1", index_key_id: "index-process", index_key_ref: "index-key.bin", current_data_key_id: "data-process", data_keys: [{ key_id: "data-process", file_ref: "data-key.bin" }] });
  write("owner-slots.json", { schema_version: "ascension.context-console.owner-slots.v1", slots: [{
    owner_id: ownerId, reference: "owner-slot-process", console_issuer: issuer, console_subject: subject, console_audience: audience, console_credential_id: principal.credential_id,
    revoked: false, harness_subject: "profile:console-live", ...scope, expires_at: expires,
    exact_harness_scopes: ["workflow:read", "workflow:context:content:read", "workflow:content:write", "workflow:context:edit", "workflow:context:objective:edit", "workflow:control"],
    bearer_file_ref: "owner-bearer.bin", bearer_sha256: crypto.createHash("sha256").update(harnessBearer).digest("hex"),
  }] });
  const configPath = path.join(root, "operator.json");
  write("operator.json", {
    schema_version: "ascension.context-console.owner-service.v1", protected_source: "protected-private-files-v1",
    listen_address: address, harness_address: harnessAddress, owner_id: ownerId, scope, issuer, audience,
    expected_host: address, expected_origin: `http://${address}`, csrf_secret_ref: "csrf.bin", state_root: root,
    principal_registry_ref: "principals.json", principal_mac_key_ref: "principal-mac.bin", owner_slots_ref: "owner-slots.json",
    journal_key_manifest_ref: "journal-keys.json", grant_database: "grants.sqlite3", journal_database: "journal.sqlite3", request_deadline_ms: 5000,
  });
  function command(name, input) {
    assert.ok(!child, "operator grant commands require the exclusive process lock");
    write("grant-input.json", input);
    const result = execFileSync(binary, ["owner", name, "--config", configPath, "--input", path.join(root, "grant-input.json")], { env, encoding: "utf8", timeout: 10_000 });
    fs.unlinkSync(path.join(root, "grant-input.json"));
    return result;
  }
  function provisionGrant(name, permission) {
    command("grant-provision", { schema_version: "ascension.context-console.subject-grant-provision.v1", grant_id: `grant-${name}`, issuer, subject, permission, scope, not_before: Math.floor(Date.now() / 1000) - 1, expires_at: expires });
  }
  for (const [name, permission] of Object.entries(permissions)) provisionGrant(name, permission);
  async function start() {
    assert.ok(!child);
    stdout = ""; stderr = "";
    let spawnError;
    child = spawn(binary, ["owner", "serve", "--config", configPath], { env, stdio: ["ignore", "pipe", "pipe"] });
    child.once("error", (error) => { spawnError = error; });
    child.stdout.on("data", (chunk) => { stdout = `${stdout}${chunk}`.slice(-32768); });
    child.stderr.on("data", (chunk) => { stderr = `${stderr}${chunk}`.slice(-32768); });
    const deadline = Date.now() + 10000;
    while (!stdout.includes('"status":"ready"')) {
      if (spawnError) throw new Error("operator could not start", { cause: spawnError });
      if (child.exitCode !== null || child.signalCode !== null) throw new Error(`operator exited before readiness: ${stderr}`);
      if (Date.now() >= deadline) throw new Error("operator readiness deadline exceeded");
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    return child.pid;
  }
  async function stop() {
    if (!child) return;
    const current = child;
    if (Number.isInteger(current.pid) && current.exitCode === null && current.signalCode === null) {
      const closed = once(current, "close");
      current.kill("SIGTERM");
      const kill = setTimeout(() => current.kill("SIGKILL"), 2000);
      try { await closed; } finally { clearTimeout(kill); }
    }
    history.push(stdout, stderr);
    child = undefined;
  }
  function invocation(operation, binding, grant = "read") {
    return {
      schema_version: "ascension.console.context-owner-invocation.v2",
      identity: {
        console: { issuer, subject, audience, credential_id: principal.credential_id, grant_id: `grant-${grant}`, grant_generation: 0, grant_expires_at: expires },
        console_scope: scope,
        harness: { actor_subject: "profile:console-live", owner_id: ownerId, workflow_run_id: runId, credential_reference_id: "owner-slot-process", credential_expires_at: expires },
      },
      ...(binding ? { expected_binding: binding } : {}), operation: { workflow_run_id: runId, ...operation },
    };
  }
  const headers = { authorization: `Bearer ${token}`, origin: `http://${address}`, "x-csrf-token": csrf };
  return {
    root, address, start, stop, invocation, provisionGrant,
    send: (body, suffix = "", overrideHeaders = {}) => request(address, `/v2/context-owner/invocations${suffix}`, { method: "POST", headers: { ...headers, ...overrideHeaders }, body }),
    revokeGrant: (grant) => command("grant-revoke", { schema_version: "ascension.context-console.subject-grant-revoke.v1", grant_id: `grant-${grant}` }),
    revokePrincipal: () => { principal.revoked = true; writePrincipal(); },
    assertPrivate(sentinel) {
      const journalFiles = fs.readdirSync(root).filter((name) => name.startsWith("journal.sqlite3"));
      assert.ok(journalFiles.length > 0, "the process witness must inspect an actual durable journal");
      for (const name of journalFiles) {
        const bytes = fs.readFileSync(path.join(root, name));
        for (const secret of [sentinel, token, csrf, harnessBearer]) assert.ok(!bytes.includes(Buffer.from(secret)), `journal ${name} contains private bytes`);
        assert.equal(fs.statSync(path.join(root, name)).mode & 0o777, 0o600);
      }
      const logs = [...history, stdout, stderr].join("\n");
      for (const secret of [sentinel, token, csrf, harnessBearer]) assert.ok(!logs.includes(secret), "operator output contains private bytes");
    },
    async close() { await stop(); fs.rmSync(root, { recursive: true, force: true }); },
  };
}
module.exports = { createOperator };
