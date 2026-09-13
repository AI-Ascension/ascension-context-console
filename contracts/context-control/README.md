# Context-control contract pin

These schemas are the versioned Phase 2 wire contract consumed by the target API and browser
fixture. They are copied from the implementation package at the Phase 2 integration boundary;
the package is specification input, while the Rust types and tests in this repository are the
executable contract.

The target source pin is the merged Phase 1 main commit
2e1bfe0d4e62ac7f1efcefe4b1cc940182146880. The companion harness source pin is
780f2d521508a2aadc76c4d779544d967955f102. Both are recorded here so a preview or receipt can
be traced to the exact adapter and contract revision used by the fixture.

The copied files are immutable artifacts. Their SHA-256 values are:

    boundary.schema.json       6feba2081c253bb1b3e36c35ad39bafbccee965dd1c7877d20515f098dd4eeed
    capabilities.schema.json   842ed93344ede334b0ce462e5c92c9cfe6240eb4146f579137310c49bab30025
    command.schema.json        0315317ec1b859a869bbd19f0cae797d9a4accab8589c6c4d07c2b8dd671d308
    control-api.openapi.json   88ec2ecc1c5f2c19e3f30ecf46fe3a928406e5986f533732973308472df26df5
    draft.schema.json          e12b6229974bc0a1e15c51027089e836910bd63d4e3178ff249e3496ef20dd5c
    event.schema.json          913a5b9331f9e3aa31b4d5839830236fc2eadd7dde4fce7ae4a8fdfd8828f794
    patch.schema.json          e3d4d9e8c7b1ceb6869f10c83df491483f15e02e0a477b6cfaec080457a2a251
    preview.schema.json        3f45a47bcf357f6825e4e9905dafe8a467d846f7f66ff51263af63d1af42bb8f
    receipt.schema.json        77ad67b6af1db42db8d41301c4201d9e2c5fe1839469880afc83111fd8578cd0
    relation.schema.json       00166fd3f7aac5568a7749f203e8d57a759b75dfbcdfd9ecac1e156024bfe73d
    revision.schema.json       13f3ba52ba152ce7c5d0a0dcfa3ccb5d2b1a4947d475a55b585cbb4e14646ff1
    state.schema.json          e52f0d342f638bcab28f8aa665af45ccf046a031826e2dae21b0fad85d3ad463

## Harness-backed facade additions

The non-demo `HarnessBackedContextService` uses the existing control records while adding a
consumer/auth boundary. These artifacts are target-owned and proposed for the external
`sts2-harness#100` owner integration:

- `harness-facade-capabilities` (JSON Schema) — filtered owner capabilities and explicit
  `unknown`/`owner_conditional` disclosure;
- `harness-facade-error` (JSON Schema) — stable code/retryability envelope with no upstream body;
- `harness-facade-auth` (JSON Schema) — deployment scope, exact Host/Origin, opaque owner-auth/CSRF
  references, and retention policy shape (never secret values; resolved before Rust construction);
- `harness-facade.openapi.json` — the same `/v2/runs/{run_id}/context-control/` paths for Studio
  consumers.

These files do not repin or claim the unsettled harness owner interface. The Rust
`HarnessOwnerPort` is the compatibility seam until that owner contract is accepted.
