# Phase 4 capability report

The checked-in `codex-app-server-fixture-v1` profile uses an owned stdio child, denies tools and
ambient history, rejects unknown methods, and keeps provider context opaque. Its evidence class is
`compiled_peer`; its native binary and schema digests identify the synthetic peer/envelope only.

The required development-agent hierarchy could not be observed: the native collaboration stream
failed with `Encrypted function output content could not be decrypted or decoded` on two attempts.
This is an orchestration blocker, not evidence that the product profile is supported by a native
Codex binary. Real binary/fake-upstream, authentication, compaction semantics and remote cleanup
are intentionally unverified.
