# Independent Verification — Context Console Module-Layout Refactor (WS-9)

Verifier: independent (read-only). Repo: `/home/agent/sts2-project/context/phase1-preflight-current/ascension-context-console`.
Branch: `refactor/context-console-module-layout` (HEAD `3ee19f5`, no new commits).
Date: 2026-09-12. Contract: orchestration prompt §1–§7 + `docs/decisions/0002-interface-freeze.md`.

Baseline inputs: `/home/agent/context-baseline/` (original binaries `context-reader-baseline`,
`context-console-baseline`, `diff-report.txt`, `files.txt`, `public-symbols.txt`,
`fixture-digest.txt`, `*.log`). LEAD's `final-*.log` were ignored. All commands below were
re-run by the verifier from fresh shells; raw verifier logs are under
`/home/agent/context-baseline/verify-scratch/`.

Two independent builds were used: the repo's own `target/` (literal gate commands) and a
fully fresh separate `CARGO_TARGET_DIR=/home/agent/context-baseline/verify-scratch/target`
(forced full recompile) to rule out stale-cache effects. Results agreed.

---

## 1. Gate results (prompt §6)

| # | Command (from REPO) | Result | Evidence |
|---|---|---|---|
| 1 | `cargo run --locked --package repo-policy -- --strict` | `Policy check: required files and language boundaries passed`, `EXIT=0` | `verify-scratch/policy.log` |
| 2 | `cargo fmt --all -- --check` | `EXIT=0` (no output) | `verify-scratch/fmt.log` |
| 3 | `cargo clippy --workspace --all-targets --locked -- -D warnings` | `Finished ... EXIT=0`; also clean on a forced fresh rebuild (16.4s) | `verify-scratch/clippy.log`, `fresh-clippy.log` |
| 4 | `cargo test --workspace --all-targets --locked` | `EXIT=0`, **72 passed / 0 failed**; identical on forced fresh rebuild | `verify-scratch/test.log`, `fresh-test.log` |
| 5 | `cargo run --locked --package context-reader -- fixtures/valid/snapshot-cli.json` | `EXIT=0`, 7 projection lines (byte-identical to baseline `reader.log`) | `verify-scratch/reader.log` |
| 6 | `cargo run --locked --package context-service --bin context-console -- demo` | `EXIT=0`, all 12 demo lines (byte-identical to baseline `demo.log`) | `verify-scratch/demo.log` |
| 7 | `timeout 3 cargo run --locked --package context-service --bin context-console -- integrated-demo 0` | `integrated_demo_ready=http://127.0.0.1:<port>/web/`, `integrated_demo_provider_calls=0`, `integrated_demo_game_launches=0`, `EXIT=124` (timeout by design) | `verify-scratch/integrated.log` |
| 8 | `node --check tools/browser-audit/integrated_browser_audit.cjs` | `EXIT=0` | `verify-scratch/nodecheck.log` |
| 9 | `sha256sum -c contract-artifact/context-inspection-v1/SHA256SUMS` (from REPO) | **fails** (`5 listed files could not be read`, `README.md FAILED`, `EXIT=1`) — same failure as pre-refactor baseline `sha.log` | `verify-scratch/sha-repo.log` |
| 9b | `cd contract-artifact/context-inspection-v1 && sha256sum -c SHA256SUMS` | all 6 entries `OK`, `EXIT=0` | `verify-scratch/sha-inside.log` |

**SHA256SUMS cwd nuance (explicit):** the §6 literal command runs from `REPO` but `SHA256SUMS`
lists bare filenames, so it can only succeed from inside `contract-artifact/context-inspection-v1/`.
The from-`REPO` invocation failed identically BEFORE the refactor (baseline `sha.log`: same 5
`No such file or directory`, same `README.md FAILED`, `EXIT=1`). The artifact bytes are therefore
unchanged; the correct verification (from inside the directory) passes for all 6 files.

Test breakdown (72 total, all green): `context_reader` lib 8, reader bin 0,
`context_reader_smoke` 1, `context_service` lib 38, `context-console` bin 0,
`integrated-demo` bin 0, `phase1_acceptance` 4, `read_api` 6, `store` 7, `repo_policy` lib 8,
`repo_policy` bin 0. Fresh rebuild produced the same counts with no warnings.

---

## 2. Static structure checks (prompt §6 + WS-9 addenda)

| Check | Command | Result | Verdict |
|---|---|---|---|
| No `#[path]` | `rg -n '#\[path' crates tools` (also repo-wide, excl `.git`/`target`) | no matches | PASS |
| No `include!` | `rg -n 'include!' crates tools` (also repo-wide) | no matches | PASS |
| No panics/unwraps in production | `rg -n 'unwrap\(|expect\(|unreachable!\(|panic!\(|todo!\(|unimplemented!\(' crates/*/src --glob '!**/tests.rs'` | no matches | PASS |
| No Python | `find . -name '*.py' -o -name '*.pyi' -o -name 'pyproject.toml' -o -name 'setup.py'` | no matches | PASS |
| Exactly one `src/lib.rs` per crate | `find crates -name lib.rs` | `crates/context-reader/src/lib.rs`, `crates/context-service/src/lib.rs` | PASS |
| No orphan flat source files | `find crates/*/src -maxdepth 1 -type f` | only the two `lib.rs` | PASS |
| Crate test dirs exist | `find crates -maxdepth 2 -name tests -type d` | `crates/context-reader/tests`, `crates/context-service/tests` | PASS |
| Root `tests/` absent | `ls -d tests` | `No such file or directory` | PASS |
| No cross-crate `[[test]] path` | `rg -n '\[\[test\]\]|path = "\.\./\.\.' Cargo.toml crates/*/Cargo.toml tools/*/Cargo.toml` | no matches (both `[[test]]` blocks removed; auto-discovery finds all 10 test targets) | PASS |
| No `unsafe` code | `rg -n 'unsafe' crates tools` | only a doc-comment word ("refuses unsafe policy setup") in `private_store/mod.rs:5`; `[workspace.lints.rust] unsafe_code = "forbid"` is active in all crates, so real `unsafe` cannot compile | PASS |
| Nested `t02-worker` gone + untracked | `ls`, `git check-ignore -v`, `git ls-files` | dir absent; `.gitignore:4:/phase1-preflight-current/` matches it; no tracked path | PASS |
| Contract hashes | see gate 9b | all `OK` | PASS |
| Frozen `pub use` surface | `rg -n 'pub use' ...` | see §3 | PASS |

Module trees exactly match the frozen layout:
- `crates/context-reader/src/`: `json/{mod,access,error,parser,value,tests}.rs`,
  `snapshot/{mod,component,error,mapping,measurement,parse,types,tests}.rs`,
  `event/{mod,details,error,parse,types,tests}.rs`, `bin/context-reader/{main,cli}.rs`.
- `crates/context-service/src/`: `http/{mod,error,framing,request,response,target,tests}.rs`,
  `store/{mod,compare,config,content,cursor,error,event,grant,query,snapshot,summary,tests}.rs`,
  `read_api/{mod,api,auth,capabilities,handlers,router,tests}.rs`,
  `capture/{mod,config,error,memory,record,sink,tests}.rs`,
  `private_store/{mod,approval,error,scope,vault,tests}.rs`,
  `telemetry/{mod,capture,error,memory,tests}.rs`,
  `demo/{mod,fixtures,routes,server,state,tests}.rs`,
  `bin/context-console/{main,cli,demo,inspect}.rs`, `bin/integrated-demo/main.rs`.

**Helper single-sourcing:**
- Reader accessors defined once, in `crates/context-reader/src/json/access.rs`
  (`keys, field, val, obj, strv, bounded, identifier, id_field, optional_id, number,
  optional_number, is_rfc3339`, `MAX_SAFE_INTEGER` at line 5, `pub(crate)`). `rg` found no second
  copy. (`Value::number` / `Parser::number` are unrelated methods.)
- Service HTTP helpers: `error_response` only `http/error.rs`, `split_target` only
  `http/target.rs`, `_epoch_now` only `http/mod.rs`, `read_request_bytes` only `http/framing.rs`.
  `demo/server.rs::read_request` is a 2-line delegate to `read_request_bytes` + `HttpRequest::parse`
  (not a re-implementation); `demo/routes.rs` imports `split_target` from `crate::http`.
- `store/` has no `TcpStream`/`std::net`/`http::` import; `http/` has no `store` import.

**File inventory:** current `find` (excluding `.git`/`target`) = 186 files vs target-tree listing.
Set-difference vs the target listing is only: `docs/decisions/0002-interface-freeze.md` present
(required by orchestration §3 as the published freeze note; §1 lists only `0001`) and root
`SECURITY.md` present (listed in §1; it is a new untracked addition — see caveats). Removed items
all gone (16 `D`/`R` entries: old flat crate files, both root `tests/*`, root `src/main.rs` files,
`observability.rs`, `web/app.js`, `web/styles.css`, `tools/integrated_browser_audit.cjs`).

`repo-policy --strict` and its 8 unit tests (incl. `required_file_list_is_unchanged`) pass
against the new tree, so the required-file list and language boundaries are behaviorally unchanged.

---

## 3. Frozen public API (§2)

A standalone scratch crate `/home/agent/context-baseline/verify-scratch/symcheck` (path-depends on
both crates, outside REPO) imports **all 18 `context_reader::*` and all 43 `context_service::*`
frozen paths** from §2. `cargo check --offline` → `Finished ... EXIT=0` (`symcheck.log`).

- `context_reader/src/lib.rs` re-exports `event::{CaptureEvent, EventDetails, EventError, EventType,
  parse_event, parse_event_lines}` and `snapshot::{CaptureMode, Component, ComponentStatus, Identity,
  Mapping, Measurement, Producer, Snapshot, SnapshotError, SnapshotProjection}`, plus
  `MAX_SNAPSHOT_BYTES = 1_048_576` and `parse_snapshot`.
- `context_service/src/lib.rs` re-exports `capture`, `demo::{demo, run as run_integrated_demo}`,
  `private_store`, `read_api::{ApiError, HttpRequest, HttpResponse, MAX_HTTP_BODY_BYTES,
  MAX_HTTP_REQUEST_BYTES, ReadApi}`, `store`, `telemetry`. `read_api/mod.rs` re-exports the four
  HTTP items from `crate::http`; `http/mod.rs` owns `ApiError/HttpRequest/HttpResponse` + both
  `MAX_HTTP_*`. `telemetry` owns the telemetry types.
- `MAX_*` values are byte-identical to HEAD: `MAX_SNAPSHOTS=128`, `MAX_MANIFEST_BYTES=1_048_576`,
  `MAX_COMPARE_BYTES=64*1024`, `MAX_EVENTS=4096`, `MAX_CONTENT_REFS=512`,
  `MAX_CONTENT_BYTES=512*1024*1024`, `MAX_SNAPSHOT_BYTES=1_048_576`.
- `Cargo.lock` unchanged ⇒ no new runtime dependencies. `Cargo.toml`s changed only for bin paths,
  `[[test]]` removal, and the additive `integrated-demo` bin. `contract-artifact/`, `fixtures/`,
  `policy.toml`, `offline-bundle.json` are byte-unchanged vs HEAD (`git diff --stat` clean).

---

## 4. Behavioral differential (baseline vs current binaries)

Harness `/home/agent/context-baseline/verify-scratch/diff.sh` runs the current
`target/debug/{context-reader,context-console}` against `context-reader-baseline` /
`context-console-baseline` over identical argv+stdin, comparing combined stdout/stderr and exit
code. Raw per-case captures in `verify-scratch/diff/`.

**Result: 29/29 MATCH, 0 DIFF** — a superset of the 23 cases in `diff-report.txt`:
- reader over `synthetic`, all 3 valid and all 10 invalid fixtures, `/nonexistent/path.json`,
  stdin (`snapshot-cli.json`), stdin (`fixture-stdin.txt`), empty stdin — 18 cases.
- console `health`, no-args, `help`, `--help`, `inspect` (cli fixture), `inspect` (metadata
  fixture), `bogus`, `demo`, `inspect` stdin, `inspect` stdin-blob, `inspect` missing — 11 cases.

Gate-output comparison:
- `reader.log` program lines vs verifier run → **IDENTICAL**.
- `demo.log` program lines vs verifier run → **IDENTICAL**.
- `integrated.log` normalized (`:[0-9]+` → `:<port>`) → **IDENTICAL** (both `EXIT=124`).

---

## 5. Browser audit (superseded by §8)

`node tools/browser-audit/integrated_browser_audit.cjs` → **FAILS to run**:
`Error: Cannot find module 'playwright'` (`MODULE_NOT_FOUND`), `EXIT=1`
(`verify-scratch/browser-audit.log`). Playwright is not resolvable in this environment
(`require.resolve('playwright')` fails; no `node_modules/playwright` under `/tmp` or the repo).
Per the task this is recorded as **unverified** — no rendered-DOM, screenshot, or
storage/CSP/adversarial-payload evidence was produced.

Supplementary (not a substitute for the browser audit) HTTP smoke test of a live
`integrated-demo` server (`verify-scratch/http-smoke.cjs`, log `http-smoke.log`) confirmed the
relocated asset layout is served correctly:

```
/web/                 200 text/html; charset=utf-8 4647
/web/index.html       200 text/html; charset=utf-8 4647
/web/css/styles.css   200 text/css; charset=utf-8 3393
/web/js/app.js        200 text/javascript; charset=utf-8 1588
/web/js/api.js        200 text/javascript; charset=utf-8 706
/web/js/bundle.js     200 text/javascript; charset=utf-8 1544
/web/js/render.js     200 text/javascript; charset=utf-8 4982
/offline-bundle.json  200 application/json 144
/demo/snapshot        200 application/json 2995
/demo/events          200 application/x-ndjson 3375
/demo/comparison      200 application/json 3185
/demo/metrics         200 application/json 227
/web/app.js           404   (old flat path)
/web/styles.css       404   (old flat path)
```

The audit script's `source_artifacts` now reference the new `crates/context-service/src/demo/*.rs`
and `web/js/*`, `web/css/*` paths (old `integrated_demo.rs` reference is gone). This cannot be
fully exercised without Playwright.

---

## 6. Per-requirement verdicts (§7 Definition of Done)

| Requirement | Verdict | Evidence |
|---|---|---|
| Target tree §1 matches; removed items gone | PASS (with 2 noted extras) | §2; `git status`; `find` |
| Frozen public API compiles/re-exports unchanged | PASS | §3; `symcheck.log` |
| Existing CLI surface + stdout/exit codes unchanged | PASS | §4 (29/29) |
| Fixture accept/reject unchanged | PASS | §4 reader cases |
| Contract artifact hashes verify | PASS (from correct cwd) | gate 9b |
| Reader accessors exist once in `json/access.rs` | PASS | §2 helper check |
| Service HTTP framing exists once in `http/`; demo reuses it | PASS | §2 helper check |
| No `#[path]`, `include!`, cross-crate `[[test]]` | PASS | §2 |
| No Python; no real `unsafe`; no panics/unwraps in prod | PASS | §2 |
| All §6 gates pass; `VERIFICATION.md` present and honest | PASS (browser audit excepted) | §1, §5 |
| Nothing committed/pushed/merged/deployed | PASS | no commits since 2026-09-10; `git log --since=2026-09-12` empty; all changes unstaged in working tree |

---

## 7. Unverified / unsupported / caveats

1. **Browser audit — unverified at the time of this run (superseded by §8).** `playwright` is not installed, so no browser run, screenshots,
   rendered-DOM assertions, storage/CSP checks, or adversarial-payload checks were performed.
   `docs/evidence/*.json` were therefore **not** regenerated.
2. **`docs/evidence/*.json` still name old paths** (`web/app.js`, `web/styles.css`,
   `crates/context-service/src/integrated_demo.rs`, `tools/integrated_browser_audit.cjs`). These are
   revision-pinned historical records (`revision` 3889e9e / 06c74f6) with matching SHA-256 fields;
   editing them by hand would falsify the record. They are consistent with "legitimately still
   reference old paths" and are not treated as stale documentation.
3. **`integrated-demo` full run not observed to completion.** It blocks by design; verified only
   under `timeout 3` with the ephemeral port normalized. The `EXIT=124` and the three ready lines
   match baseline.
4. **Non-ASCII preserved in source.** `crates/context-service/src/demo/mod.rs:3` contains a `→`
   character in a doc comment. It is **pre-existing** (identical line in HEAD
   `crates/context-service/src/integrated_demo.rs`), a verbatim move, not a new violation. `docs/`
   prose was ASCII-normalized by WS-8; this source doc comment was not.
5. **Baseline `files.txt` is internally inconsistent.** It lists `web/css/styles.css` (post-move)
   even though git HEAD contains `web/styles.css`; it also omits root `SECURITY.md`. The verifier
   therefore used the git HEAD tree + the baseline *binaries* for the differential, not
   `files.txt`. No behavioral or structural conclusion depends on `files.txt`.
6. **One extra file beyond the §1 literal listing:** `docs/decisions/0002-interface-freeze.md`,
   the published Interface Freeze required by orchestration §3. It is not in the §1 tree listing
   (which names only `0001-read-only-inspection-boundary.md`) but is explicitly authorized by §3.
   It is the only file present beyond the §1/expected inventory.
7. **Root `SECURITY.md` provenance.** It is listed in §1 and present, but it is a new untracked file
   relative to git HEAD; it was not in the pre-refactor tree. It is also not in `repo-policy`'s
   DOC001 required list (which references `docs/SECURITY.md`), so the policy gate is unaffected.
8. **`raw code diff equivalence` not independently recomputed line-by-line.** The verifier relied on
   the behavioral differential (29/29) and the symbol/constant checks rather than per-line AST
   comparison; the worker-supplied verbatim-diff reports were not re-derived.

---

## 8. Update — mainline integration browser audit (2026-09-12, revision b03f2f5)

Supersedes §5 and §7 items 1-2 for the merged `main` revision.

Playwright 1.63.0 is available from the cached module path and Chromium 1243 is installed; the
headless system libraries are staged under `/tmp/ascension-browser-libs/root-20260911`. The browser
audits were re-run at merge revision `b03f2f56af05549cf33b43a95be2c4217ecb06fa` (`main`):

```bash
export LD_LIBRARY_PATH=/tmp/ascension-browser-libs/root-20260911/usr/lib/x86_64-linux-gnu:/tmp/ascension-browser-libs/root-20260911/lib/x86_64-linux-gnu
export FONTCONFIG_FILE=/tmp/ascension-browser-libs/fontconfig-runtime-20260911.conf
export FONTCONFIG_PATH=/tmp/ascension-browser-libs/root-20260911/etc/fonts
export XDG_DATA_DIRS=/tmp/ascension-browser-libs/root-20260911/usr/share:/usr/local/share:/usr/share
export PLAYWRIGHT_MODULE=/home/agent/.npm/_npx/e41f203b7505f1fb/node_modules/playwright

node tools/browser-audit/integrated_browser_audit.cjs   # EXIT=0
node tools/phase2_browser_audit.cjs                      # EXIT=0
node tools/phase4_session_loopback.cjs                   # EXIT=0
```

- Integrated audit: `result=passed`, `exit_code=0`, Playwright 1.63.0, browser 153.0.8010.12,
  all 18 assertions true (adversarial text-only payload, manifest path rejection, keyboard
  comparison, reduced motion, no browser persistence, no narrow overflow, memory-disabled shadow,
  phase4 session fixture, CSRF denied, async-state distinctness).
- Refreshed evidence: `docs/evidence/integrated-browser-ui-20260910.json` (revision b03f2f5, no
  stale paths; `source_artifacts` point at `demo/*.rs` and `web/js/*`), plus
  `phase2-browser-ui-20260910.json`, the two phase2 screenshots, and
  `phase4-session-loopback-20260911.json` (all `passed`, zero provider/game/external calls).
- §7 item 1 (browser audit unverified) and item 2 (evidence still names old paths) no longer apply
  to the current `main` revision.
