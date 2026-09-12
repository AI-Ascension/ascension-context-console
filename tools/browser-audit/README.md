# Integrated browser audit

Read-only Playwright audit for the Context Console web surface. It starts the local
`context-console` integrated demo, drives `/web/` in headless Chromium, and checks normal,
adversarial (markup injection), and manifest path-rejection flows. It writes screenshots and a
JSON evidence file under `docs/evidence/`.

## Usage

From the repository root:

```bash
node tools/browser-audit/integrated_browser_audit.cjs
```

`package.json` declares no runtime dependencies beyond Node built-ins. The script resolves
Playwright at run time from `PLAYWRIGHT_MODULE` when set, otherwise from the ambient module
resolution path; provide a Playwright installation that is external to this repository.

## Environment variables

- `PLAYWRIGHT_MODULE` - module path or name used to resolve Playwright (default: `playwright`).
- `CONTEXT_CONSOLE_BIN` - prebuilt demo binary; when set the audit runs it directly instead of
  `cargo run`.
- `SHOW_SERVER_STDERR` - set to `1` to echo the demo server's stderr.
- `FONTCONFIG_PATH`, `FONTCONFIG_FILE`, `XDG_DATA_DIRS`, `LD_LIBRARY_PATH` - recorded in the
  evidence file to describe the headless browser environment.

## Evidence

The audit writes `docs/evidence/integrated-browser-ui-20260910.json` plus desktop and narrow
screenshots into `docs/evidence/`. Evidence files are owned by the docs workstream; do not edit
them by hand.

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
