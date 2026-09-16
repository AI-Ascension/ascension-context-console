# Integrated browser audit

Read-only Playwright audit for the Context Console web surface. It starts the local
`context-console` integrated demo, drives `/web/` in headless Chromium, and checks normal,
adversarial (markup injection), and manifest path-rejection flows. It writes screenshots and a
JSON evidence file in a newly created temporary directory, or in the explicit
`CONTEXT_BROWSER_AUDIT_OUT` directory. Historical `docs/evidence/` files are not overwritten.
The saved-policy journey runs in a separate browser page against a strict synthetic contract
fixture for the same-origin Harness routes. It checks the authenticated request shape, exact
uploaded bytes, initial adoption, proposal approval and adoption. It does not establish that a
deployment routes those paths to a live Harness owner.

## Usage

Use Node 24.16.0 and the exact locked Playwright dependency. From the repository root:

```bash
cargo build --locked --package context-service --bin context-console
npm ci --prefix tools/browser-audit
cd tools/browser-audit
npx playwright install --with-deps chromium
CONTEXT_CONSOLE_BIN="../../target/debug/context-console" npm run audit
```

`package-lock.json` binds Playwright 1.63.0 and its dependency integrity. CI provisions
Node explicitly and builds the demo before starting the 30-second readiness timer.
The script resolves Playwright locally unless `PLAYWRIGHT_MODULE` is explicitly set
for a diagnostic environment; such an override must be recorded in its evidence.

## Environment variables

- `PLAYWRIGHT_MODULE` - module path or name used to resolve Playwright (default: `playwright`).
- `CONTEXT_CONSOLE_BIN` - prebuilt demo binary; when set the audit runs it directly instead of
  `cargo run`.
- `CONTEXT_BROWSER_AUDIT_OUT` - disposable output directory; unset creates a new temporary directory.
- `SHOW_SERVER_STDERR` - set to `1` to echo the demo server's stderr.
- `FONTCONFIG_PATH`, `FONTCONFIG_FILE`, `XDG_DATA_DIRS`, `LD_LIBRARY_PATH` - recorded in the
  evidence file to describe the headless browser environment.

## Evidence

The audit writes `integrated-browser-ui.json` plus desktop and narrow screenshots into
its disposable output directory. Do not point it at historical evidence. Browser traffic
is restricted to the demo's loopback origin, and the owned server is stopped in cleanup
even if closing the browser fails. These tests exercise the local integrated demo, not
native provider/harness interoperability.

## Headless prerequisites

The audit needs a Playwright module that matches the cached Chromium build and the usual headless
Chromium shared libraries. When they are not installed system-wide, point the environment at the
cached module and a staged library root, for example:

```bash
export LD_LIBRARY_PATH=/tmp/ascension-browser-libs/root-20260911/usr/lib/x86_64-linux-gnu:/tmp/ascension-browser-libs/root-20260911/lib/x86_64-linux-gnu
export FONTCONFIG_FILE=/tmp/ascension-browser-libs/fontconfig-runtime-20260911.conf
export FONTCONFIG_PATH=/tmp/ascension-browser-libs/root-20260911/etc/fonts
export XDG_DATA_DIRS=/tmp/ascension-browser-libs/root-20260911/usr/share:/usr/local/share:/usr/share
export PLAYWRIGHT_MODULE=/home/agent/.npm/_npx/e41f203b7505f1fb/node_modules/playwright
node tools/browser-audit/integrated_browser_audit.cjs
```
