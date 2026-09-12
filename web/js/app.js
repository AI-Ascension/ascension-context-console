// SPDX-License-Identifier: MIT

import { loadEvents, loadJson } from "./api.js";
import { loadBundle } from "./bundle.js";
import { render, validateSnapshot } from "./render.js";

const status = document.querySelector("#status");

function showError(message) {
  status.textContent = "Offline bundle unavailable";
  status.dataset.state = "error";
  document.querySelector("#error-message").textContent = message;
  document.querySelector("#error").hidden = false;
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
}

loadFixture().catch((error) => {
  showError(error instanceof Error ? error.message : "bundle could not be read");
});
