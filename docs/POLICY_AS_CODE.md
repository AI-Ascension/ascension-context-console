# Policy as code

`tools/repo-policy` is a bounded, read-only Rust checker. It verifies required repository
documents, every file digest declared in the copied context-inspection manifest,
safe artifact-relative paths, and the prohibition on Python source/package metadata. It
does not claim to prove runtime behavior, encryption, authorization, browser safety or provider
fidelity; those require focused tests and review.

`ARTIFACT001` reports missing/malformed manifests, missing files, unsafe paths and
SHA-256 mismatches. Run `cargo run --locked --package repo-policy -- --strict`
from the repository root; `policy.yml` already invokes this gate. Updating a copied
contract requires a reviewed manifest and producer provenance, not merely resealing
changed bytes. Local digest verification does not prove historical producer source
availability or live harness compatibility.
