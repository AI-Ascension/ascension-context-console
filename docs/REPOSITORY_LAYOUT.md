# Repository layout

`crates/context-reader` contains pure bounded manifest validation and projections.
`crates/context-service` contains local store, read authorization and the CLI/API boundary.
`contract-artifact/context-inspection-v1` is the pinned schema copy. `fixtures/` contains original
synthetic records. `web/` is a small static read-only frontend. `tests/` holds acceptance and
integration checks. `tools/integrated_browser_audit.cjs` drives the provider-free integrated
producer/capture/API/browser demonstration, and `tools/repo-policy` provides the local policy gate.
