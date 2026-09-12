// SPDX-License-Identifier: MIT

const MAX_FIXTURE_BYTES = 1024 * 1024;

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
