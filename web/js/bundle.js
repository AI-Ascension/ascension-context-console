// SPDX-License-Identifier: MIT

import { loadJson } from "./api.js";

const bundleManifestUrl = new URL("../offline-bundle.json", document.baseURI);

function resolveBundleArtifact(value, field) {
  const hasControlOrWhitespace = typeof value === "string"
    && [...value].some((character) => {
      const codePoint = character.codePointAt(0);
      return codePoint <= 0x20 || codePoint === 0x7f;
    });
  if (typeof value !== "string" || value.length === 0 || value.length > 256
    || hasControlOrWhitespace || value.startsWith("/") || value.includes("\\") || value.includes("..")
    || value.includes("%") || value.includes("://")) {
    throw new Error(`offline bundle ${field} path is invalid`);
  }
  const url = new URL(value, bundleManifestUrl);
  if (url.origin !== window.location.origin || url.search || url.hash) {
    throw new Error(`offline bundle ${field} URL is invalid`);
  }
  return url;
}

export function validateBundleManifest(manifest) {
  if (!manifest || typeof manifest !== "object" || manifest.schema !== "ascension.offline-bundle.v1"
    || manifest.evidence !== "synthetic") {
    throw new Error("unsupported offline bundle manifest");
  }
  return {
    snapshotUrl: resolveBundleArtifact(manifest.snapshot, "snapshot"),
    compareUrl: resolveBundleArtifact(manifest.comparison, "comparison"),
    eventsUrl: resolveBundleArtifact(manifest.events, "events"),
  };
}

export async function loadBundle() {
  return validateBundleManifest(await loadJson(bundleManifestUrl));
}

export { bundleManifestUrl };
