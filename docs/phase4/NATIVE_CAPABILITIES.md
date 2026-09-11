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

Native-binary execution remains unverified. Starting the installed binary against personal/default
state was not authorized, and no isolated fake-upstream credential/config lane was available. The
required development-agent hierarchy was attempted: depth 1 observed `gpt-5.6-luna`/`max`, but its
native callable registry exposed no child-spawn, reservation, or close operation, so depth 2 and
depth 3 ancestry could not be established. This is an orchestration and isolation limitation, not
evidence that the native profile is supported. Authentication, native compaction semantics,
provider transforms, persistence, and remote cleanup remain unverified.
