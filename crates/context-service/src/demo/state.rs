// SPDX-License-Identifier: MIT

//! In-memory state behind the loopback demonstration server.
//!
//! The state is rebuilt per process from the checked-in fixtures. It counts producer, capture,
//! browser, and API activity so the `/demo/metrics` route can prove the transport stayed local.

use super::fixtures;
use super::{COMPARISON_ID, DURABLE_STORE_KEY, PROJECT, RUN, SNAPSHOT_ID, TOKEN, durable_error};
use crate::capture::{CaptureConfig, CaptureMode, CaptureSink, MemoryCapture, PreparedCapture};
use crate::control::{ControlError, ControlPlane, DurableControlStore};
use crate::memory::{MemoryRoute, MemoryScope};
use crate::provider_session::ProviderSessionRoute;
use crate::store::{CapturePrivilege, IngestError, ReadGrant, Store};
use std::fs;
use std::path::PathBuf;
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
    pub(super) control: ControlPlane,
    pub(super) control_store: DurableControlStore,
    pub(super) control_store_path: PathBuf,
    pub(super) memory: MemoryRoute,
    pub(super) provider_sessions: ProviderSessionRoute,
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
        let mut control = ControlPlane::synthetic();
        let nonce = now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let control_store_path = std::env::temp_dir().join(format!(
            "ascension-context-console-control-{}-{nonce}.sqlite",
            std::process::id()
        ));
        let mut control_store =
            DurableControlStore::create(&control_store_path, DURABLE_STORE_KEY, &control)
                .map_err(|_| IngestError::Capacity)?;
        control_store
            .copy_phase1_snapshot(SNAPSHOT_ID, producer_snapshot)
            .map_err(|_| IngestError::Capacity)?;
        control_store
            .copy_phase1_snapshot(COMPARISON_ID, producer_comparison)
            .map_err(|_| IngestError::Capacity)?;
        drop(control_store);
        let control_store = DurableControlStore::open(
            &control_store_path,
            DURABLE_STORE_KEY,
            control.scope().run_id.clone(),
        )
        .map_err(|_| IngestError::Capacity)?;
        control = control_store.load().map_err(|_| IngestError::Capacity)?;
        let control_scope = control.scope().clone();
        let mut memory = MemoryRoute::new(
            MemoryScope {
                project_id: control_scope.project_id,
                run_id: control_scope.run_id,
                episode_id: control_scope.episode_id,
                agent_id: control_scope.agent_id,
            },
            false,
        );
        memory.grant_search("fixture-editor-token");
        memory.grant_review("fixture-objective-token");
        let provider_sessions = ProviderSessionRoute::fixture(super::SESSION_PRINCIPAL);
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
            control,
            control_store,
            control_store_path,
            memory,
            provider_sessions,
        })
    }

    pub(super) fn capabilities(&self) -> crate::control::Capabilities {
        let mut capabilities = self.control.capabilities();
        capabilities.durable_control_store = "supported".to_owned();
        capabilities
    }

    pub(super) fn mutate_control<T>(
        &mut self,
        mutation: impl FnOnce(&mut ControlPlane) -> Result<T, ControlError>,
    ) -> Result<T, ControlError> {
        let mut candidate = self.control.clone();
        let result = mutation(&mut candidate)?;
        self.control_store
            .persist(&candidate)
            .map_err(durable_error)?;
        self.control = candidate;
        Ok(result)
    }
}

impl Drop for DemoState {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.control_store_path);
        let _ = fs::remove_file(self.control_store_path.with_extension("sqlite-wal"));
        let _ = fs::remove_file(self.control_store_path.with_extension("sqlite-shm"));
    }
}
