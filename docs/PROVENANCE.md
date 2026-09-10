# Bootstrap provenance

`fixtures/synthetic/snapshot.json` is an original, nonprivate fixture for T02. It describes a
metadata-only HTTP adapter boundary and intentionally carries no prompt, model output, credential,
absolute path or provider event stream. Its producer revision is the accepted
`AI-Ascension/sts2-harness` source commit:

```text
fc44d3ef65fefa6d13ecd5f690e5335a6ef60080
```

The fixture's `evidence` value is `synthetic`. It demonstrates schema-shaped input and incomplete
content status, not a successful provider call or game run. The reader validates this distinction:
`application_capture_complete` cannot be true when a component is metadata-only, and unavailable
measurements remain null.

No provider, game, gateway, MCP server or external store is launched by this bootstrap.
