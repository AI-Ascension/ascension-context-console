# Security Policy

This repository is a private, read-only Phase 1 context inspector. The accepted security boundary
is documented in `docs/SECURITY.md`; that document is authoritative.

- Bind local services to loopback and require exact Host/Origin values.
- Read grants are scoped, short-lived, revocable and held in process memory.
- Tokens never appear in URLs, committed files or browser persistent storage.
- Metadata and content privileges are separate.
- Reject traversal, arbitrary URLs, unknown routes, oversized manifests, duplicate identities and
  conflicting bytes.
- Never downgrade unsafe private storage to plaintext.
- Capture is default-off and fails closed.

Report vulnerabilities privately through the organization security channel. Do not include
credentials, private traces or exploit material in reports.
