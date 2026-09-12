// SPDX-License-Identifier: MIT

const MAX_FIXTURE_BYTES = 1024 * 1024;
const CONTROL_BASE = "/v2/runs/fixture-run/context-control";
const MEMORY_BASE = "/v3/memory";
const SESSION_BASE = "/v1/runs/fixture-run-001";
const CONTROL_TOKEN = "fixture-editor-token";
const OBJECTIVE_TOKEN = "fixture-objective-token";
const SESSION_TOKEN = "fixture-session-token";
const CSRF_TOKEN = "fixture-csrf-token";

async function boundedText(response) {
  if (!response.ok) throw new Error(`bundle request returned ${response.status}`);
  const text = await response.text();
  if (new TextEncoder().encode(text).length > MAX_FIXTURE_BYTES) throw new Error("bundle exceeds its byte bound");
  return text;
}

export async function loadJson(url) {
  return JSON.parse(await boundedText(await fetch(url, { cache: "no-store" })));
}

export async function loadEvents(eventsUrl) {
  const text = await boundedText(await fetch(eventsUrl, { cache: "no-store" }));
  return text.split("\n").filter((line) => line.trim()).map((line) => JSON.parse(line));
}

export async function controlJson(path, options = {}) {
  const { token = CONTROL_TOKEN, ...requestOptions } = options;
  const headers = new Headers(requestOptions.headers || {});
  headers.set("Authorization", `Bearer ${token}`);
  if (requestOptions.method && requestOptions.method !== "GET") headers.set("X-CSRF-Token", CSRF_TOKEN);
  const response = await fetch(`${CONTROL_BASE}${path}`, { ...requestOptions, headers, cache: "no-store" });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error?.message || value.error?.code || `control request returned ${response.status}`);
  return value;
}

export async function memoryJson(path, options = {}) {
  const { token = CONTROL_TOKEN, ...requestOptions } = options;
  const headers = new Headers(requestOptions.headers || {});
  headers.set("Authorization", `Bearer ${token}`);
  if (requestOptions.method && requestOptions.method !== "GET") headers.set("X-CSRF-Token", CSRF_TOKEN);
  const response = await fetch(`${MEMORY_BASE}${path}`, { ...requestOptions, headers, cache: "no-store" });
  const value = await response.json();
  if (!response.ok) throw new Error(value.error || `memory request returned ${response.status}`);
  return value;
}

export async function sessionJson(path, options = {}) {
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

export { CONTROL_TOKEN, OBJECTIVE_TOKEN };
