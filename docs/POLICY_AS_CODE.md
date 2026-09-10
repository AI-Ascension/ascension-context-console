# Policy as code

`tools/repo-policy` is a bounded, read-only Rust checker. It verifies required repository
documents, the contract artifact path and the prohibition on Python source/package metadata. It
does not claim to prove runtime behavior, encryption, authorization, browser safety or provider
fidelity; those require focused tests and review.
