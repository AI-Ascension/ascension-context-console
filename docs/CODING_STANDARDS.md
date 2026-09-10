# Coding standards

Use Rust 1.97.1, edition 2024, locked dependencies, `cargo fmt`, warnings-denied Clippy and
workspace tests. Keep production modules cohesive, bounded and free of `unsafe`, panics and
provider/game access. Validate untrusted input at boundaries and use typed errors.

Keep all identifiers namespaced and distinct. Preserve optional, null, ordering, byte-size and
unknown measurement semantics. Do not store credentials, private prompts, model output, saves,
personal paths or hidden reasoning. Browser output uses text nodes and a strict same-origin CSP.

Claims carry `confirmed`, `source-derived`, `proposed`, `inferred`, `unverified` or `unsupported`
evidence labels. A synthetic fixture or local parse does not prove provider, game or native
compatibility.
