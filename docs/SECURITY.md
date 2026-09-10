# Security

Bind local services to loopback and require exact Host/Origin values. Read grants are scoped,
short-lived, revocable and held in process memory; tokens never appear in URLs, committed files or
browser persistent storage. Metadata and content privileges are separate.

Reject traversal, arbitrary URLs, unknown routes, oversized manifests, duplicate identities and
conflicting bytes. Never downgrade unsafe private storage to plaintext. Capture failures are
bounded and fail-soft for the producer. Report vulnerabilities privately through the organization
security channel; do not include credentials, private traces or exploit material.
