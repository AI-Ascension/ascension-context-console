# Phase 4 capability report

The checked-in `codex-app-server-fixture-v1` profile uses an owned stdio child and the JSON-RPC
2.0 line envelope used by the installed App Server schema. It denies tools and ambient history,
rejects unknown methods, and keeps provider context opaque. Its evidence class is `compiled_peer`;
the fixture binary is not a native Codex binary.

Local discovery on 2026-09-11 found `/tmp/codex-account1-bin/codex`, version `codex-cli 0.154.0`,
with binary SHA-256
`3188814c35471432d4123203e0eb38e5bddc60226e3d7ddf0e59e649ea140022`. The local
`app-server generate-json-schema` command completed without inference and produced a bundle digest
`9045dc90eb460503ec0420fcc8452f110038f6d211b3d26ae9c77e7aa27be1bb`. The reviewed native method
names present in that schema are `initialize`, `thread/start`, `thread/read`, `turn/start`,
`turn/interrupt`, `thread/fork`, and `thread/compact/start`; local retirement remains a broker
operation because no `thread/retire` method is present in the schema.
The compiled fixture capability reports `encrypted_state: false`; persistent `enabled` operation is
therefore unavailable until a native encrypted boundary is independently verified.

The harness now exposes an explicit broker-owned metadata store with `volatile` and
`encrypted_persistent` modes. The persistent mode authenticates an XChaCha20-Poly1305 envelope,
uses a private owner-checked path with atomic replacement, and restores only after exact
scope/policy/profile matching. This protects the bounded harness journal (epochs, idempotency,
retirements and redacted projections); it does not contain or attest to native Codex state,
rollouts, WAL files, logs or temporary files. Native encrypted-state durability therefore remains
unverified.

The owned stdio transport now clears the parent environment and binds conventional `HOME`,
`CODEX_HOME`, XDG, Windows profile and temporary roots to the approved state root; callers cannot
override those names through the inherited-environment list. The child still needs an independently
verified native profile and OS-level containment before this source boundary can be promoted to
persistent native capability. Startup scans the complete private state tree against the 256 MiB
bound and fails closed on symlinks, special files, unsafe child roots or over-limit bytes; the walk
is also capped at 65,536 entries and 32 directory levels. Before each allowlisted request and after
each response, the owned transport rescans the tree; an overage fences and terminates the owned
child, with a compiled-fixture growth test covering the post-start path. The compiled fixture
reports the bound-root invariant at initialization; these checks do not establish the installed
native binary's own precedence behavior or runtime growth behavior.
Only the approved `OPENAI_API_KEY` secret name may cross the inherited-environment boundary; path,
endpoint and unrelated credential names are rejected by configuration validation.
The owned transport rejects a forbidden method before writing a frame; this is a broker/fixture
allowlist guarantee, not proof that an installed native binary will suppress its own built-in tools.
An unapproved automatic context transform observed during an in-flight turn is fenced by the
broker: the attempt is retained as unknown, the binding is held and the history epoch advances so
the late result and stale prepared input cannot regain authority. This is a broker-level response to
a reported transform; the owned transport counts native notifications without parsing their method,
so wire-level native transform detection remains unverified.
Broker retention is finite at every admission and restore boundary: policy validation caps completed
turns at 128 and scope lifetime at 24 hours, history refresh/completion/restore enforce the selected
turn and aggregate-byte limits, and fork-created evaluation candidates plus combined fork/compaction
maintenance jobs consume the four-candidate and two-job quotas. Expired or over-quota records are
rejected rather than retained by a pin or a restore.

An isolated native smoke was run with a disposable state root and a loopback-refused proxy. It
observed successful `initialize`, ephemeral `thread/start`, and metadata-only `thread/read`
responses, with an idle thread, no instruction sources, and the native `:read-only` profile. The
machine-readable record is
[`phase4-native-isolated-smoke-20260911.json`](../evidence/phase4-native-isolated-smoke-20260911.json).

A second controlled lane configured the installed binary with a synthetic `fake` model provider
whose Responses endpoint was a local loopback HTTP/SSE server. It completed two streamed turns and
read the idle thread metadata; a durable one-turn run wrote a rollout and recovered it by ID after a
process restart. Additional probes accepted a durable-source ephemeral fork with excluded turns and
accepted `thread/compact/start` against the local fake provider; the latter produced a second
completed turn but did not expose an explicit context-compacted event or prove transformed context.
The complete bounded record is
[`phase4-native-fake-conformance-20260911.json`](../evidence/phase4-native-fake-conformance-20260911.json).
The default native request forwarded 11 built-in tool definitions on each turn. A follow-up restricted
profile probe used supported feature/config switches and forwarded only the residual
`request_user_input` tool, with no forbidden shell, file, code, browser, MCP, skill, subagent or
background-terminal tool definitions observed. This is partial hardening evidence: the residual
interactive tool, absent OS-level egress proof, and untested server-initiated elicitation denial keep
the native hardening profile unavailable for the default product path. The fork and compaction probes
are endpoint observations, not clean-rehydration or transform proofs.
A provider-generated `request_user_input` callback denial attempt produced no native server request;
the router repeatedly reported that the residual tool was unavailable in Default mode, so no clean
denial response was established. A read-only `migrate-rollouts --json --verbose` dry run on an empty
disposable state root returned zero outcomes; it did not exercise migration of persisted state.
A fully disabled native fake profile then forwarded zero tool definitions and completed a synthetic
turn; a forged `exec` function-call response produced no server request or execution. The request
still contained a bounded runtime developer envelope, and the binary warned that bubblewrap was
unavailable, so this is profile-level tool suppression rather than complete OS/instruction
containment. A Linux tmpfs variant kept all 78 regular native state files on tmpfs (four helper
symlinks pointed only to the installed executable) and wrote no `history.jsonl`; that proves the
explicit volatile setup in this environment, not encrypted persistent storage or cross-platform
spill resistance.
The required development-agent hierarchy was attempted: depth 1 observed `gpt-5.6-luna`/`max`, but
its native callable registry exposed no child-spawn, reservation, or close operation, so depth 2 and
depth 3 ancestry could not be established.

The operator subsequently relaxed the exact-settings requirement to "use the controls actually
available". A fresh preflight then spawned a real depth-1 descendant through the runtime's native
agent control; that child reported its tool surface contained no spawn, reservation, messaging,
resume or close operation, so no depth-2 or depth-3 ancestry could be created. No ancestry was
simulated and no model or effort was substituted. The runtime does not expose a child's model or
effort, so the model/effort facts stay unverified and the orchestration-success claim remains
withheld rather than fabricated. The machine-readable record is
[`phase4-native-orchestration-and-envelope-20260912.json`](../evidence/phase4-native-orchestration-and-envelope-20260912.json).

A separate local probe used `codex debug prompt-input` (no inference, no network) with a disposable
home/config root and an emptied environment. The installed binary still placed a runtime developer
envelope — `skills_instructions`, `multi_agent_role` and `multi_agent_mode`, plus `permissions`,
`collaboration_mode` and `environment_context` by default — into model-visible input. The `include_*`
and feature switches removed the permissions, collaboration-mode and environment sections but not the
skills or multi-agent developer messages, and that residual persisted with an empty environment.
Environment isolation alone therefore cannot prove a clean model-visible envelope; the raw
instruction text is intentionally not retained. A companion OS probe found `unshare -n` denied and
`codex sandbox --sandbox-state-disable-network` unusable without the `codex/sandbox-state-meta`
payload, so OS egress containment remains unproven.

Authentication realm, ambient-history exclusion, encrypted-state guarantees, migration, native
compaction/transforms, and remote cleanup remain unverified; the tool-denial failure is recorded
above.
