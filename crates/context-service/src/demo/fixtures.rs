// SPDX-License-Identifier: MIT

//! Checked-in synthetic bytes served by the demonstration.
//!
//! Every asset is embedded at compile time from the repository tree. Nothing here performs a
//! provider, game, URL, or process lookup; the demonstration only replays local fixtures.

pub(super) const CLI_SNAPSHOT: &[u8] =
    include_bytes!("../../../../fixtures/valid/snapshot-cli.json");
pub(super) const METADATA_SNAPSHOT: &[u8] =
    include_bytes!("../../../../fixtures/valid/snapshot-metadata.json");
pub(super) const EVENTS: &[u8] = include_bytes!("../../../../fixtures/valid/events.jsonl");

pub(super) const BLOB_STDIN: &[u8] = include_bytes!("../../../../fixtures/blobs/fixture-stdin.txt");
pub(super) const BLOB_OUTPUT_SCHEMA: &[u8] =
    include_bytes!("../../../../fixtures/blobs/fixture-output-schema.json");
pub(super) const BLOB_CONFIGURATION: &[u8] =
    include_bytes!("../../../../fixtures/blobs/fixture-configuration.json");
pub(super) const BLOB_HTTP_BODY: &[u8] =
    include_bytes!("../../../../fixtures/blobs/fixture-http-body.json");

pub(super) const WEB_INDEX: &[u8] = include_bytes!("../../../../web/index.html");
pub(super) const WEB_STYLES: &[u8] = include_bytes!("../../../../web/css/styles.css");
pub(super) const WEB_APP: &[u8] = include_bytes!("../../../../web/js/app.js");
pub(super) const WEB_API: &[u8] = include_bytes!("../../../../web/js/api.js");
pub(super) const WEB_BUNDLE: &[u8] = include_bytes!("../../../../web/js/bundle.js");
pub(super) const WEB_RENDER: &[u8] = include_bytes!("../../../../web/js/render.js");
