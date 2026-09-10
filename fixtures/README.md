# Original synthetic interchange fixtures

All data here is original, synthetic and nonprivate. Fixture model names, actions, counts and timestamps are not real provider/game evidence. The main-source revision identifies the design's inspected source pin, not an executable that produced these fixtures.

The valid CLI fixture captures original stdin text, output schema and nonsecret configuration. The HTTP fixture captures an original serialized chat body and an omitted upstream field. The metadata fixture contains no content references. Event fixtures illustrate local write evidence, numeric usage, one attempt referenced by multiple actions, expiration and a gap. They intentionally contain no provider-receipt event; local writing does not prove receipt.

`blob-index.json` maps opaque fixture references to packaged bytes and SHA-256 for independent parity checks. `cases.json` declares positive/negative schema and semantic vectors. Duplicate component IDs/ordinals, bad content hashes and unknown mapping references require semantic checks beyond JSON Schema. The invalid raw-reasoning field is a tiny synthetic rejection marker, not actual hidden reasoning.

These static contract fixtures are not a replacement for real harness-valid request fixtures or actual-process testing. Implementation must generate compatible observations/actions with current validated source builders. No real request capture, credentials, private output, game asset, keyfile or executable is supplied.
