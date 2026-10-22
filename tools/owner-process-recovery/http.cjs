// SPDX-License-Identifier: MIT
const assert = require("node:assert/strict");
const http = require("node:http");
const crypto = require("node:crypto");

async function listen(server) {
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return `127.0.0.1:${server.address().port}`;
}

async function close(server) {
  if (!server.listening) return;
  const closed = new Promise((resolve) => server.close(resolve));
  server.closeAllConnections?.();
  await closed;
}

function request(address, path, { method = "GET", headers = {}, body } = {}) {
  const bytes = body === undefined ? undefined : Buffer.from(JSON.stringify(body));
  return new Promise((resolve, reject) => {
    const call = http.request(`http://${address}${path}`, {
      method, agent: false,
      headers: { ...headers, ...(bytes ? { "content-type": "application/json", "content-length": bytes.length } : {}) },
    }, (response) => {
      const chunks = [];
      let length = 0;
      response.on("data", (chunk) => {
        length += chunk.length;
        if (length > 1024 * 1024) call.destroy(new Error("bounded witness response exceeded"));
        else chunks.push(chunk);
      });
      response.on("error", reject);
      response.on("end", () => {
        try {
          resolve({ status: response.statusCode, value: JSON.parse(Buffer.concat(chunks).toString("utf8")) });
        } catch { reject(new Error("witness response was not bounded JSON")); }
      });
    });
    const timer = setTimeout(() => call.destroy(new Error("witness HTTP deadline exceeded")), 10_000);
    call.once("close", () => clearTimeout(timer));
    call.once("error", reject);
    call.end(bytes);
  });
}

function successful(response, label) {
  assert.equal(response.status, 200, `${label}: status ${response.status}, code ${response.value?.error?.code ?? response.value?.error ?? "none"}`);
  return response.value;
}

// This test-only forwarder delivers requests to the real Harness. It can lose
// one complete response only AFTER the owner has replied successfully. It
// retains method/path/body digests, never authorization headers or raw bodies.
async function recordingForwarder(ownerAddress) {
  const calls = [];
  let armed;
  let lost;
  const server = http.createServer((incoming, outgoing) => {
    // Preserve the owner's closed framing contract; Node otherwise injects a
    // Date extension header that the production Console correctly rejects.
    outgoing.sendDate = false;
    const chunks = [];
    let length = 0;
    incoming.on("data", (chunk) => {
      length += chunk.length;
      if (length > 1024 * 1024) incoming.destroy();
      else chunks.push(chunk);
    });
    incoming.on("end", () => {
      const body = Buffer.concat(chunks);
      const record = { method: incoming.method, path: incoming.url, digest: crypto.createHash("sha256").update(body).digest("hex") };
      calls.push(record);
      const drop = armed && armed.method === record.method && armed.path === record.path;
      if (drop) armed = undefined;
      const forwarded = http.request(`http://${ownerAddress}${incoming.url}`, {
        method: incoming.method, agent: false,
        headers: { ...incoming.headers, host: ownerAddress, connection: "close" },
      }, (response) => {
        const reply = [];
        let size = 0;
        response.on("data", (chunk) => {
          size += chunk.length;
          if (size > 1024 * 1024) forwarded.destroy();
          else reply.push(chunk);
        });
        response.on("error", () => outgoing.destroy());
        response.on("end", () => {
          const bytes = Buffer.concat(reply);
          record.status = response.statusCode;
          const value = JSON.parse(bytes.toString("utf8"));
          if (typeof value?.error?.code === "string" && /^[a-z0-9_.:-]{1,128}$/.test(value.error.code)) record.errorCode = value.error.code;
          if (drop) {
            lost = { ...record, value };
            outgoing.destroy();
          } else {
            outgoing.writeHead(response.statusCode, response.headers);
            outgoing.end(bytes);
          }
        });
      });
      forwarded.setTimeout(8_000, () => forwarded.destroy());
      forwarded.on("error", () => outgoing.destroy());
      forwarded.end(body);
    });
  });
  return {
    address: await listen(server), calls,
    loseNext(method, path) { assert.equal(armed, undefined); lost = undefined; armed = { method, path }; },
    lostResponse() { assert.ok(lost, "the actual owner must have replied before the connection was lost"); return lost; },
    close: () => close(server),
  };
}

module.exports = { listen, close, request, successful, recordingForwarder };
