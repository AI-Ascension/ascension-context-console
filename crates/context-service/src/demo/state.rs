// SPDX-License-Identifier: MIT

//! In-memory state behind the loopback demonstration server.
//!
//! The state is rebuilt per process from the checked-in fixtures. It counts producer, capture,
//! browser, and API activity so the `/demo/metrics` route can prove the transport stayed local.

use super::fixtures;
use super::{PROJECT, RUN, SNAPSHOT_ID, TOKEN};
use crate::capture::{CaptureConfig, CaptureMode, CaptureSink, MemoryCapture, PreparedCapture};
use crate::store::{CapturePrivilege, IngestError, ReadGrant, Store};
use std::time::{Duration, SystemTime};

pub(super) struct DemoState {
    pub(super) store: Store,
    pub(super) grant: ReadGrant,
    pub(super) token: Vec<u8>,
    pub(super) expected_host: String,
    pub(super) expected_origin: String,
    pub(super) producer_snapshots: usize,
    pub(super) capture_records: usize,
    pub(super) producer_events: usize,
    pub(super) browser_requests: usize,
    pub(super) api_requests: usize,
}

impl DemoState {
    pub(super) fn build(port: u16) -> Result<Self, IngestError> {
        let producer_snapshot = fixtures::METADATA_SNAPSHOT;
        let producer_comparison = fixtures::CLI_SNAPSHOT;
        let producer_events = fixtures::EVENTS;

        // Producer stage: the bytes are the checked-in synthetic projection. Capture stage:
        // MemoryCapture receives those exact bytes before the store validates and retains them.
        let mut capture = MemoryCapture::new(CaptureConfig {
            mode: CaptureMode::Memory,
            max_queue_entries: 16,
            max_record_bytes: 1_048_576,
        })
        .map_err(|_| IngestError::Capacity)?;
        capture
            .prepared(PreparedCapture {
                snapshot_id: SNAPSHOT_ID,
                attempt_id: "attempt-fixture-001",
                boundary: "adapter.cli_input",
                bytes: producer_snapshot,
            })
            .map_err(|_| IngestError::Capacity)?;
        capture
            .write_completed(SNAPSHOT_ID)
            .map_err(|_| IngestError::Capacity)?;

        let mut store = Store::default();
        store.ingest(producer_snapshot)?;
        store.ingest(producer_comparison)?;
        let mut producer_event_count = 0_usize;
        for line in producer_events
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            store.append_event(line)?;
            producer_event_count += 1;
        }
        let now = SystemTime::now();
        let grant = ReadGrant::issue(
            TOKEN,
            PROJECT,
            Some(RUN.to_owned()),
            CapturePrivilege::Content,
            Duration::from_secs(3600),
            now,
        )
        .map_err(|_| IngestError::Capacity)?;
        Ok(Self {
            store,
            grant,
            token: TOKEN.to_vec(),
            expected_host: format!("127.0.0.1:{port}"),
            expected_origin: format!("http://127.0.0.1:{port}"),
            producer_snapshots: 2,
            capture_records: capture.records().count(),
            producer_events: producer_event_count,
            browser_requests: 0,
            api_requests: 0,
        })
    }
}
