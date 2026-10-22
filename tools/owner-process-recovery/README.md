# Independent Console owner recovery

This local process witness exercises the actual `context-console owner serve` private-file
composition against the production Harness HTTP owner. Its exact Harness, Gateway and MCP source
tuple is recorded separately in `contracts/owner-process-recovery.lock.json`. The older
`contracts/live-owner-browser.lock.json` and its browser acceptance profile remain separate.

The witness covers authenticated association/limits, populated draft listing, revision-page
decoding, scoped draft edits, pause receipts, and exact receipt recovery. A bounded test forwarder
drops one reply only after the real owner has completed the write and returned its receipt. Console
is stopped, Harness is restarted, and Console is started again. Receipt lookup and cached results
must equal that original receipt; neither route may repeat the write. Current principal and grant
revocation still prevent cached disclosure. A newly provisioned metadata grant can authorize the
same original recovery operation without changing its payload or stable key.

Wrong Host, Origin, identity, scope, CSRF, edit/objective and resume grants are refused before owner
dispatch. The test checks the actual encrypted journal and captured operator output for private
note text and throwaway credentials. Its protected identity files intentionally contain the local
test credentials, remain private, and are deleted during teardown. No credentials enter arguments,
source control, or recorded request metadata.

## Reproduce

Use Rust 1.97.1 and Node 24.16.0. Check out the three repositories at the exact lock revisions under
`harness/`, `gateway/`, and `mcp/`, respecting each repository's worktree instructions. Source
checkouts must be clean: the fixture checks every HEAD and dirty status before starting. Build the
four production binaries and the isolated helper, with locked dependency resolution:

```sh
cargo build --locked --package context-service --bin context-console
cargo build --locked --manifest-path harness/Cargo.toml --bin sts2-harness-runtime
cargo build --locked --manifest-path gateway/Cargo.toml --bin sts2-gateway-runtime
cargo build --locked --manifest-path mcp/Cargo.toml --bin sts2-mcp-server
cargo build --locked --manifest-path tools/owner-process-fixture/Cargo.toml
```

The helper uses the existing policy fixture source with its own lockfile for this producer tuple;
it does not modify the older browser helper. Set the absolute binary paths if the build uses a
different target directory. From the Console root:

```sh
export CONSOLE_POLICY_HARNESS_ROOT="$PWD/harness"
export CONSOLE_POLICY_GATEWAY_ROOT="$PWD/gateway"
export CONSOLE_POLICY_MCP_ROOT="$PWD/mcp"
export CONSOLE_POLICY_HARNESS_RUNTIME_BINARY="$PWD/harness/target/debug/sts2-harness-runtime"
export CONSOLE_POLICY_GATEWAY_BINARY="$PWD/gateway/target/debug/sts2-gateway-runtime"
export CONSOLE_POLICY_MCP_BINARY="$PWD/mcp/target/debug/sts2-mcp-server"
export CONSOLE_POLICY_FIXTURE_BINARY="$PWD/tools/owner-process-fixture/target/debug/console-owner-process-fixture"
export CONTEXT_CONSOLE_BIN="$PWD/target/debug/context-console"
export CONSOLE_OWNER_PRIVATE_TEST_ROOT="$PWD/target/owner-process-private"
install -d -m 700 "$CONSOLE_OWNER_PRIVATE_TEST_ROOT"
node --test tools/owner-process-recovery/recovery.test.cjs
```

The private parent must be service-owned and have no group/world-writable ancestor. On a shared
checkout under an unsafe writable ancestor, select an explicitly owned private test directory;
keep the production checks intact. The CI job `Independent Console owner recovery` builds and runs
the same tuple without a browser download. Each test owns its temporary private child directory
and processes and removes them during teardown.

## Evidence boundary

The production processes use a synthetic Mod and a bounded local provider bridge. The tested
read, draft, pause and recovery operations must leave both effect counters unchanged. This is
local process evidence; it does not establish native game behavior, an external provider result,
deployment, key custody, or completion of Console #18.

The broader held-edit/preview/commit/resume sequence remains open. On the frozen tuple a preview
without captured trusted render input is unavailable. A subsequent workflow Step after context
pause reaches the local provider bridge; its zero-provider-effect assertion failed and is retained
as separate producer regression evidence. This witness does not call Step to manufacture preview
authority. Historical receipt recovery also does not establish a fresh current binding after an
owner restart. Those acceptance obligations remain under #18.
