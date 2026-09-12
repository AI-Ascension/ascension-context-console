// SPDX-License-Identifier: MIT

const MAX_EVENTS = 256;
const status = document.querySelector("#status");

function showText(selector, value) {
  const node = document.querySelector(selector);
  node.textContent = value == null || value === "" ? "unavailable" : String(value);
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
