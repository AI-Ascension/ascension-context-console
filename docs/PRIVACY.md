# Privacy boundary

The reader and console receive only application-controlled records selected by a harness capture
seam. Provider-added context, hidden instructions, credentials, cookies, authorization headers,
stderr, raw event streams, and provider reasoning are not accepted by the snapshot/event contracts.

Metadata mode keeps allowlisted identities, sizes, statuses, mappings, and qualified measurement
labels. It does not keep component bytes or digests. Memory mode is opt in and process bounded.
Private mode is opt in, requires an accepted policy and restricted authorization, and encrypts bytes
with XChaCha20-Poly1305 using the project, snapshot, component, and content reference as associated data; unsafe setup cannot fall
back to plaintext. The plaintext snapshot store rejects private-mode content references until the
approved vault is wired to that boundary.

Read capabilities are held in memory, scoped to a project and optional run, expire, and can be
revoked. The API rejects credentials in URLs and does not expose persistent browser storage. The
browser uses same-origin resources, a restrictive CSP, `textContent`, and no external images or
scripts. Host administrators may still inspect an authorized process's memory; this tool does not
claim to remove that authority.
