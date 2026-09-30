# Pinned producer conformance vectors

`producer.json` is original synthetic data emitted by the actual `sts2-harness` library at
`27f1d4219e98999df4903615a57236e4747ae932`. The consumer-owned generator is
[`tools/effective-limit-fixtures`](../../tools/effective-limit-fixtures). Its exact Git dependency
and separate lockfile make regeneration independent of sibling working trees. No producer
implementation is copied into this repository, and the Console product has no harness dependency.

Fixture SHA-256: `3c2d78f163683cb1cc2b079cd003332889facec2fad708560a93b9679a8af0cf`.
Generator lockfile SHA-256:
`fd1d7c33d586d3cf54f550a219b64dfd87f4d4c46468d72e6bd570b5fa0cae5c`.

The six vectors cover memory and provider sessions at default, restricted and disabled settings.
Restricted memory starts with `MemoryCorpus::with_limits(scope, 16, 4096)`; the generator also
reduces every other public descriptor ceiling. Restricted session reduces every public ceiling.
The producer recomputes descriptor digests and validates both restricted descriptors. These are
admissible synthetic contract configurations, not evidence of provisioned runtime profiles.

The disabled memory descriptor comes from `MemoryCorpus::set_enabled(false)`. The disabled
session descriptor deliberately clears the synthetic peer's methods: the producer rejects that
descriptor for native attachment, but its own record derivation yields `enabled: false`. The
vector records `producer_descriptor_valid: false` and tests disabled record admission only.
It must never be counted as a valid native provider profile.

The consumer tests use its actual dual readers and record derivation/admission methods. They
compare every row with the independently emitted producer record and copied schema ceilings,
exercise exact/over-limit values and mutations, check payload digests, and preserve legacy v1
reading. A parsed descriptor is not authenticated owner data; the pinned fixture supplies the
independent trust anchor in these tests. Real owner authentication remains a composition gate.

## Regeneration

From the repository root with pinned Rust 1.97.1:

```sh
mkdir -p target/console24
cargo run --locked --manifest-path tools/effective-limit-fixtures/Cargo.toml \
  > target/console24/producer.json
cmp fixtures/effective-limits/producer.json target/console24/producer.json
cargo test --locked -p context-service --test producer_conformance
```

The generator invokes no provider, native peer, game, listener or deployed service. Compilation
may fetch its exact Git dependencies and locked crates. Its fixture's `compiled_peer` metadata
comes from the producer's fixture constructor; it does not mean this generator launched a peer.

The coordinated cutover defaults to v3; explicit v1 rollback reports a pending consumer pin and
omits effective-limit claims. `capability_routes` exercises the actual attached route composition
with these records and separately configured trust descriptors. Studio v3 adoption and the
harness-owned authoritative pin-matrix update remain merge gates. These vectors establish
producer-library synthetic conformance only; see [ADR 0021](../../docs/decisions/0021-owner-capability-sidecar.md).

## Session vectors are `v4`; memory vectors are still `v3`

The generator pins `sts2-harness` at a `v4` producer revision, so the session cases carry
`ascension.provider-session.capabilities.v4` bytes and the matching `v4`
`binding.policy_schema_sha256`. The context-memory surface was not part of the rename and its
vectors remain `ascension.context-memory.capabilities.v3`; the two surfaces are deliberately not on
the same schema revision, and `console_consumer_pin_for` reflects that.

The fixture bytes are never hand-edited to match. The digests are self-authenticating and
`inputs::equal_fixture` compares producer output byte for byte, so the only way to move the vectors
is to repin the generator's `sts2-harness` Git dependency, re-lock, and re-run it. That repin is
root-owned under this repository's `AGENTS.md`.
