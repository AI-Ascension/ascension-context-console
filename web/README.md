# Context Console web surface

Static, read-only inspector served from this directory. The page reads only the same-origin
`offline-bundle.json` manifest and the same-origin synthetic management control, memory, and
provider-session fixtures. No build step is required: the files are served as-is.

## Layout

- `index.html` - single page shell; loads `js/app.js` as a module and `css/styles.css`.
- `js/app.js` - bootstrap, DOM wiring, and the management, memory, and provider-session flows.
- `js/api.js` - same-origin fetch wrapper with a byte bound and `cache: "no-store"` for the
  offline bundle and the fixture control, memory, and provider-session requests.
- `js/bundle.js` - `offline-bundle.json` loading and strict artifact path validation. Absolute,
  escaped, encoded, and cross-origin paths are rejected before any request is issued.
- `js/render.js` - text-node rendering of the snapshot projection, comparison, and the
  management control, memory, and provider-session panels.
- `css/styles.css` - presentation only.

## Guarantees

- Same-origin only: no external resources, no credential URLs, no provider or game calls.
- No `localStorage`, `sessionStorage`, `IndexedDB`, or Cache Storage use.
- Fixture text is written with `textContent`, never interpreted as markup.
- Management control, memory, and provider-session requests target only the local synthetic
  fixtures; the console never launches a game or calls a provider.
- The strict Content-Security-Policy in `index.html` stays in force (`default-src 'self'`).

Server routes that serve these files are owned by `crates/context-service`; the browser audit
lives in `tools/browser-audit`.
