// SPDX-License-Identifier: MIT

use context_reader::{CaptureEvent, Snapshot, SnapshotProjection, parse_event};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_SNAPSHOTS: usize = 128;
pub const MAX_MANIFEST_BYTES: usize = 1_048_576;
pub const MAX_COMPARE_BYTES: usize = 64 * 1024;
pub const MAX_EVENTS: usize = 4096;
pub const MAX_CONTENT_REFS: usize = 512;
pub const MAX_CONTENT_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturePrivilege {
    Metadata,
    Content,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadGrant {
    token_digest: [u8; 32],
    project: String,
    run: Option<String>,
    privilege: CapturePrivilege,
    expires_at: u64,
}

impl ReadGrant {
    pub fn issue(
        token: &[u8],
        project: impl Into<String>,
        run: Option<String>,
        privilege: CapturePrivilege,
        ttl: Duration,
        now: SystemTime,
    ) -> Result<Self, ReadError> {
        let project = project.into();
        if !valid_id(&project) {
            return Err(ReadError::InvalidScope);
        }
        if run.as_deref().is_some_and(|value| !valid_id(value)) {
            return Err(ReadError::InvalidScope);
        }
        if token.is_empty() || token.len() > 256 {
            return Err(ReadError::InvalidToken);
        }
        let expires_at = epoch_seconds(now)
            .checked_add(ttl.as_secs())
            .ok_or(ReadError::Expired)?;
        let token_digest: [u8; 32] = Sha256::digest(token).into();
        Ok(Self {
            token_digest,
            project,
            run,
            privilege,
            expires_at,
        })
    }

    fn permits_scope(&self, snapshot: &SnapshotProjection) -> bool {
        self.project == snapshot.identity.agent_id
            && self
                .run
                .as_deref()
                .is_none_or(|run| run == snapshot.identity.run_id)
    }

    pub fn privilege(&self) -> CapturePrivilege {
        self.privilege
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn run(&self) -> Option<&str> {
        self.run.as_deref()
    }

    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    fn valid_token(&self, token: &[u8], now: SystemTime) -> bool {
        if epoch_seconds(now) >= self.expires_at || token.is_empty() || token.len() > 256 {
            return false;
        }
        let digest: [u8; 32] = Sha256::digest(token).into();
        digest == self.token_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreConfig {
    pub max_snapshots: usize,
    pub max_manifest_bytes: usize,
    pub max_compare_bytes: usize,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            max_snapshots: MAX_SNAPSHOTS,
            max_manifest_bytes: MAX_MANIFEST_BYTES,
            max_compare_bytes: MAX_COMPARE_BYTES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SnapshotSummary {
    pub snapshot_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub boundary: String,
    pub capture_mode: String,
    pub component_count: usize,
    pub application_capture_complete: bool,
    pub incomplete_reasons: Vec<String>,
}

impl SnapshotSummary {
    fn from(snapshot: &Snapshot) -> Self {
        let projection = snapshot.projection();
        Self {
            snapshot_id: projection.snapshot_id.clone(),
            run_id: projection.identity.run_id.clone(),
            episode_id: projection.identity.episode_id.clone(),
            boundary: projection.boundary.clone(),
            capture_mode: projection.capture_mode.as_str().to_owned(),
            component_count: projection.component_count,
            application_capture_complete: projection.application_capture_complete,
            incomplete_reasons: projection.incomplete_reasons.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ComponentSummary {
    pub snapshot_id: String,
    pub component_id: String,
    pub ordinal: u64,
    pub kind: String,
    pub role: Option<String>,
    pub media_type: String,
    pub observed_bytes: u64,
    pub content_status: String,
    pub content_available: bool,
    pub digest_present: bool,
    pub measurement: context_reader::Measurement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventPage {
    pub events: Vec<CaptureEvent>,
    pub next_cursor: Option<u64>,
    pub gap: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CompareResult {
    pub left_snapshot_id: String,
    pub right_snapshot_id: String,
    pub same_boundary: bool,
    pub same_component_order: bool,
    pub changed_components: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum IngestError {
    Empty,
    TooLarge,
    InvalidSnapshot(String),
    Capacity,
    Conflict,
    InvalidEvent(String),
    EventConflict,
    ContentConflict,
    PrivateContentUnsupported,
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("manifest is empty"),
            Self::TooLarge => formatter.write_str("manifest exceeds configured bound"),
            Self::InvalidSnapshot(message) => write!(formatter, "snapshot rejected: {message}"),
            Self::Capacity => formatter.write_str("snapshot capacity is full"),
            Self::Conflict => formatter.write_str("snapshot identity already has different bytes"),
            Self::InvalidEvent(message) => write!(formatter, "event rejected: {message}"),
            Self::EventConflict => {
                formatter.write_str("event identity already has different bytes")
            }
            Self::ContentConflict => {
                formatter.write_str("content reference already has different bytes")
            }
            Self::PrivateContentUnsupported => formatter
                .write_str("private snapshot content cannot be retained in the plaintext store"),
        }
    }
}

impl std::error::Error for IngestError {}

#[derive(Debug, Eq, PartialEq)]
pub enum ReadError {
    InvalidToken,
    InvalidScope,
    Expired,
    Forbidden,
    NotFound,
    TooLarge,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidToken => "invalid read token",
            Self::InvalidScope => "invalid read scope",
            Self::Expired => "read grant expired",
            Self::Forbidden => "read not permitted",
            Self::NotFound => "snapshot unavailable",
            Self::TooLarge => "comparison exceeds its bound",
        })
    }
}

impl std::error::Error for ReadError {}

#[derive(Default)]
pub struct Store {
    config: StoreConfig,
    entries: BTreeMap<String, StoredSnapshot>,
    events: BTreeMap<String, StoredEvent>,
    event_scope_sequences: BTreeMap<(String, String), u64>,
    content: BTreeMap<String, Vec<u8>>,
    content_bytes: usize,
    revoked: BTreeMap<[u8; 32], ()>,
}

struct StoredSnapshot {
    digest: [u8; 32],
    bytes: Vec<u8>,
    snapshot: Snapshot,
}

struct StoredEvent {
    digest: [u8; 32],
    event: CaptureEvent,
    scope: Option<(String, String)>,
    scope_sequence: Option<u64>,
}

impl Store {
    pub fn with_config(config: StoreConfig) -> Result<Self, IngestError> {
        if config.max_snapshots == 0
            || config.max_snapshots > MAX_SNAPSHOTS
            || config.max_manifest_bytes == 0
            || config.max_manifest_bytes > MAX_MANIFEST_BYTES
            || config.max_compare_bytes == 0
            || config.max_compare_bytes > MAX_COMPARE_BYTES
        {
            return Err(IngestError::Capacity);
        }
        Ok(Self {
            config,
            entries: BTreeMap::new(),
            events: BTreeMap::new(),
            event_scope_sequences: BTreeMap::new(),
            content: BTreeMap::new(),
            content_bytes: 0,
            revoked: BTreeMap::new(),
        })
    }

    pub fn ingest(&mut self, bytes: &[u8]) -> Result<SnapshotSummary, IngestError> {
        if bytes.is_empty() {
            return Err(IngestError::Empty);
        }
        if bytes.len() > self.config.max_manifest_bytes {
            return Err(IngestError::TooLarge);
        }
        let snapshot = Snapshot::parse(bytes)
            .map_err(|error| IngestError::InvalidSnapshot(error.to_string()))?;
        if snapshot.projection().capture_mode == context_reader::CaptureMode::Private
            && snapshot
                .components()
                .iter()
                .any(|component| component.content_ref.is_some())
        {
            return Err(IngestError::PrivateContentUnsupported);
        }
        let id = snapshot.projection().snapshot_id.clone();
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if let Some(existing) = self.entries.get(&id) {
            if existing.digest != digest {
                return Err(IngestError::Conflict);
            }
            return Ok(SnapshotSummary::from(&existing.snapshot));
        }
        if self.entries.len() >= self.config.max_snapshots {
            return Err(IngestError::Capacity);
        }
        let summary = SnapshotSummary::from(&snapshot);
        self.entries.insert(
            id,
            StoredSnapshot {
                digest,
                bytes: bytes.to_vec(),
                snapshot,
            },
        );
        Ok(summary)
    }

    /// Append one immutable lifecycle event. Duplicate event IDs are idempotent only when their
    /// exact bytes match; conflicting reuse is rejected without changing snapshot state.
    pub fn append_event(&mut self, bytes: &[u8]) -> Result<CaptureEvent, IngestError> {
        if bytes.is_empty() {
            return Err(IngestError::InvalidEvent("event is empty".to_owned()));
        }
        if bytes.len() > self.config.max_manifest_bytes {
            return Err(IngestError::TooLarge);
        }
        let event =
            parse_event(bytes).map_err(|error| IngestError::InvalidEvent(error.to_string()))?;
        if let Some(snapshot_id) = event.snapshot_id.as_deref()
            && !self.entries.contains_key(snapshot_id)
        {
            return Err(IngestError::InvalidEvent("unknown snapshot_id".to_owned()));
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if let Some(existing) = self.events.get(&event.event_id) {
            if existing.digest != digest {
                return Err(IngestError::EventConflict);
            }
            return Ok(existing.event.clone());
        }
        if self.events.len() >= MAX_EVENTS {
            return Err(IngestError::Capacity);
        }
        let scope = event
            .snapshot_id
            .as_deref()
            .and_then(|snapshot_id| self.entries.get(snapshot_id))
            .map(|snapshot| {
                let projection = snapshot.snapshot.projection();
                (
                    projection.identity.agent_id.clone(),
                    projection.identity.run_id.clone(),
                )
            });
        let scope_sequence = scope.as_ref().map(|scope| {
            let next = self.event_scope_sequences.entry(scope.clone()).or_default();
            let sequence = *next;
            *next = next.saturating_add(1);
            sequence
        });
        self.events.insert(
            event.event_id.clone(),
            StoredEvent {
                digest,
                event: event.clone(),
                scope,
                scope_sequence,
            },
        );
        Ok(event)
    }

    /// Retain one already-classified opaque component reference in bounded memory. The store
    /// never accepts filesystem paths or URLs and treats identical re-ingest as idempotent.
    pub fn ingest_content(&mut self, content_ref: &str, bytes: &[u8]) -> Result<(), IngestError> {
        if !valid_id(content_ref) || bytes.len() > MAX_MANIFEST_BYTES.saturating_mul(16) {
            return Err(IngestError::TooLarge);
        }
        if let Some(existing) = self.content.get(content_ref) {
            return if existing == bytes {
                Ok(())
            } else {
                Err(IngestError::ContentConflict)
            };
        }
        if self.content.len() >= MAX_CONTENT_REFS
            || self.content_bytes.saturating_add(bytes.len()) > MAX_CONTENT_BYTES
        {
            return Err(IngestError::Capacity);
        }
        self.content_bytes = self.content_bytes.saturating_add(bytes.len());
        self.content.insert(content_ref.to_owned(), bytes.to_vec());
        Ok(())
    }

    pub fn component(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        snapshot_id: &str,
        component_id: &str,
        now: SystemTime,
    ) -> Result<ComponentSummary, ReadError> {
        self.authorize(token, grant, now)?;
        let entry = self.scoped_entry(grant, snapshot_id)?;
        let component = entry
            .snapshot
            .components()
            .iter()
            .find(|component| component.component_id == component_id)
            .ok_or(ReadError::NotFound)?;
        let content_available =
            self.content_available(grant, entry.snapshot.projection().capture_mode, component);
        Ok(ComponentSummary {
            snapshot_id: snapshot_id.to_owned(),
            component_id: component.component_id.clone(),
            ordinal: component.ordinal,
            kind: component.kind.clone(),
            role: component.role.clone(),
            media_type: component.media_type.clone(),
            observed_bytes: component.observed_bytes,
            content_status: component.content_status.as_str().to_owned(),
            content_available,
            digest_present: component.sha256.is_some(),
            measurement: component.measurement.clone(),
        })
    }

    pub fn content(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        snapshot_id: &str,
        component_id: &str,
        now: SystemTime,
    ) -> Result<Vec<u8>, ReadError> {
        self.authorize(token, grant, now)?;
        let entry = self.scoped_entry(grant, snapshot_id)?;
        if grant.privilege != CapturePrivilege::Content {
            return Err(ReadError::Forbidden);
        }
        let component = entry
            .snapshot
            .components()
            .iter()
            .find(|component| component.component_id == component_id)
            .ok_or(ReadError::NotFound)?;
        if !self.content_available(grant, entry.snapshot.projection().capture_mode, component) {
            return Err(ReadError::NotFound);
        }
        let content_ref = component
            .content_ref
            .as_deref()
            .ok_or(ReadError::NotFound)?;
        self.content
            .get(content_ref)
            .cloned()
            .ok_or(ReadError::NotFound)
    }

    pub fn events(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        run_id: &str,
        after_sequence: Option<u64>,
        limit: usize,
        now: SystemTime,
    ) -> Result<EventPage, ReadError> {
        if !valid_id(run_id) || limit == 0 || limit > 200 {
            return Err(ReadError::TooLarge);
        }
        self.authorize(token, grant, now)?;
        let scope_project = grant.project();
        let mut scoped_events: Vec<(u64, CaptureEvent)> = self
            .events
            .values()
            .filter_map(|stored| {
                let (project, event_run) = stored.scope.as_ref()?;
                if project != scope_project || event_run != run_id {
                    return None;
                }
                let sequence = stored.scope_sequence?;
                let mut event = stored.event.clone();
                event.sequence = sequence;
                Some((sequence, event))
            })
            .collect();
        scoped_events.sort_by_key(|(sequence, _)| *sequence);

        // Producer sequences are intentionally not exposed: they are usually global to a
        // producer, so filtering them by project/run would reveal hidden event positions. The
        // immutable append ordinal is assigned once when an event enters this run scope, so a
        // late producer sequence cannot reorder or invalidate a reconnect cursor.
        let mut events = scoped_events
            .into_iter()
            .filter_map(|(local_sequence, event)| {
                after_sequence
                    .is_none_or(|cursor| local_sequence > cursor)
                    .then_some(event)
            })
            .collect::<Vec<_>>();
        let next_cursor = if events.len() > limit {
            let next = events[limit - 1].sequence;
            events.truncate(limit);
            Some(next)
        } else {
            None
        };
        // Producer-side capture gaps remain explicit `capture.gap` events. This flag is reserved
        // for a future scope-local retention watermark.
        let gap = false;
        Ok(EventPage {
            events,
            next_cursor,
            gap,
        })
    }

    pub fn revoke(&mut self, token: &[u8]) -> Result<(), ReadError> {
        if token.is_empty() || token.len() > 256 {
            return Err(ReadError::InvalidToken);
        }
        let digest: [u8; 32] = Sha256::digest(token).into();
        self.revoked.insert(digest, ());
        Ok(())
    }

    pub fn list(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        now: SystemTime,
        limit: usize,
    ) -> Result<Vec<SnapshotSummary>, ReadError> {
        if limit == 0 || limit > 200 {
            return Err(ReadError::TooLarge);
        }
        self.authorize(token, grant, now)?;
        let mut result = Vec::new();
        for entry in self.entries.values() {
            if grant.permits_scope(entry.snapshot.projection()) {
                result.push(SnapshotSummary::from(&entry.snapshot));
                if result.len() == limit {
                    break;
                }
            }
        }
        Ok(result)
    }

    pub fn get(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        snapshot_id: &str,
        now: SystemTime,
    ) -> Result<Vec<u8>, ReadError> {
        self.authorize(token, grant, now)?;
        let entry = self.scoped_entry(grant, snapshot_id)?;
        if grant.privilege == CapturePrivilege::Metadata
            && entry
                .snapshot
                .components()
                .iter()
                .any(|component| component.content_ref.is_some())
        {
            return Err(ReadError::Forbidden);
        }
        Ok(entry.bytes.clone())
    }

    pub fn compare(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        left: &str,
        right: &str,
        now: SystemTime,
    ) -> Result<CompareResult, ReadError> {
        let left_bytes = self.get(token, grant, left, now)?;
        let right_bytes = self.get(token, grant, right, now)?;
        if left_bytes.len().saturating_add(right_bytes.len()) > self.config.max_compare_bytes {
            return Err(ReadError::TooLarge);
        }
        let left_snapshot = self.entries.get(left).ok_or(ReadError::NotFound)?;
        let right_snapshot = self.entries.get(right).ok_or(ReadError::NotFound)?;
        let left_components = left_snapshot.snapshot.components();
        let right_components = right_snapshot.snapshot.components();
        let mut changed_components = Vec::new();
        let max_components = left_components.len().max(right_components.len());
        for index in 0..max_components {
            let left_component = left_components.get(index);
            let right_component = right_components.get(index);
            let changed = match (left_component, right_component) {
                (Some(left), Some(right)) => {
                    left.component_id != right.component_id
                        || left.ordinal != right.ordinal
                        || left.observed_bytes != right.observed_bytes
                        || left.content_status != right.content_status
                        || left.sha256 != right.sha256
                }
                (Some(_), None) | (None, Some(_)) => true,
                (None, None) => false,
            };
            if changed && let Some(component) = left_component.or(right_component) {
                changed_components.push(component.component_id.clone());
            }
            if changed_components.len() >= 128 {
                break;
            }
        }
        Ok(CompareResult {
            left_snapshot_id: left.to_owned(),
            right_snapshot_id: right.to_owned(),
            same_boundary: left_snapshot.snapshot.projection().boundary
                == right_snapshot.snapshot.projection().boundary,
            same_component_order: left_components.len() == right_components.len()
                && left_components
                    .iter()
                    .zip(right_components.iter())
                    .all(|(left, right)| left.ordinal == right.ordinal),
            changed_components,
        })
    }

    pub fn compare_for_run(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        run_id: &str,
        left: &str,
        right: &str,
        now: SystemTime,
    ) -> Result<CompareResult, ReadError> {
        if !valid_id(run_id) {
            return Err(ReadError::InvalidScope);
        }
        self.authorize(token, grant, now)?;
        let left_entry = self.scoped_entry(grant, left)?;
        let right_entry = self.scoped_entry(grant, right)?;
        if left_entry.snapshot.projection().identity.run_id != run_id
            || right_entry.snapshot.projection().identity.run_id != run_id
        {
            return Err(ReadError::NotFound);
        }
        self.compare(token, grant, left, right, now)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    pub fn content_len(&self) -> usize {
        self.content.len()
    }

    fn content_available(
        &self,
        grant: &ReadGrant,
        capture_mode: context_reader::CaptureMode,
        component: &context_reader::Component,
    ) -> bool {
        let (Some(content_ref), Some(expected_digest)) = (
            component.content_ref.as_deref(),
            component.sha256.as_deref(),
        ) else {
            return false;
        };
        if grant.privilege != CapturePrivilege::Content
            || capture_mode == context_reader::CaptureMode::Private
            || matches!(
                component.content_status,
                context_reader::ComponentStatus::Expired
            )
        {
            return false;
        }
        self.content.get(content_ref).is_some_and(|bytes| {
            let digest: [u8; 32] = Sha256::digest(bytes).into();
            hex_digest(&digest) == expected_digest
        })
    }

    pub fn authorize(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        now: SystemTime,
    ) -> Result<(), ReadError> {
        if !grant.valid_token(token, now) || self.revoked.contains_key(&grant.token_digest) {
            return Err(if epoch_seconds(now) >= grant.expires_at {
                ReadError::Expired
            } else {
                ReadError::Forbidden
            });
        }
        Ok(())
    }

    fn scoped_entry(
        &self,
        grant: &ReadGrant,
        snapshot_id: &str,
    ) -> Result<&StoredSnapshot, ReadError> {
        let entry = self.entries.get(snapshot_id).ok_or(ReadError::NotFound)?;
        if grant.permits_scope(entry.snapshot.projection()) {
            Ok(entry)
        } else {
            Err(ReadError::NotFound)
        }
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}

fn epoch_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn hex_digest(bytes: &[u8; 32]) -> String {
    let mut value = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const FIXTURE: &[u8] = include_bytes!("../../../fixtures/synthetic/snapshot.json");
    const CLI_FIXTURE: &[u8] = include_bytes!("../../../fixtures/valid/snapshot-cli.json");
    const NOW: SystemTime = UNIX_EPOCH;

    fn grant(privilege: CapturePrivilege) -> ReadGrant {
        ReadGrant::issue(
            b"unit-test-token",
            "agent-t02-reader",
            None,
            privilege,
            Duration::from_secs(60),
            NOW,
        )
        .expect("valid grant")
    }

    #[test]
    fn ingest_is_idempotent_but_conflicting_bytes_are_rejected() {
        let mut store = Store::default();
        let summary = store.ingest(FIXTURE).expect("fixture");
        assert_eq!(summary.snapshot_id, "snapshot-t02-synthetic-001");
        assert_eq!(store.ingest(FIXTURE), Ok(summary.clone()));
        let mut changed = FIXTURE.to_vec();
        changed.push(b' ');
        assert!(matches!(store.ingest(&changed), Err(IngestError::Conflict)));
    }

    #[test]
    fn metadata_grant_reads_metadata_fixture_and_scope_is_checked() {
        let mut store = Store::default();
        store.ingest(FIXTURE).expect("fixture");
        let grant = grant(CapturePrivilege::Metadata);
        assert!(
            store
                .get(
                    b"unit-test-token",
                    &grant,
                    "snapshot-t02-synthetic-001",
                    NOW
                )
                .is_ok()
        );
        assert!(matches!(
            store.get(b"wrong-token", &grant, "snapshot-t02-synthetic-001", NOW),
            Err(ReadError::Forbidden)
        ));
    }

    #[test]
    fn expiry_and_revoke_are_fail_closed() {
        let mut store = Store::default();
        store.ingest(FIXTURE).expect("fixture");
        let grant = grant(CapturePrivilege::Metadata);
        assert!(matches!(
            store.get(
                b"unit-test-token",
                &grant,
                "snapshot-t02-synthetic-001",
                NOW + Duration::from_secs(60)
            ),
            Err(ReadError::Expired)
        ));
        store.revoke(b"unit-test-token").expect("revoke");
        assert!(matches!(
            store.get(
                b"unit-test-token",
                &grant,
                "snapshot-t02-synthetic-001",
                NOW
            ),
            Err(ReadError::Forbidden)
        ));
    }

    #[test]
    fn content_requires_matching_opaque_blob_and_content_privilege() {
        let mut store = Store::default();
        store.ingest(CLI_FIXTURE).expect("snapshot");
        let bytes = include_bytes!("../../../fixtures/blobs/fixture-stdin.txt");
        store
            .ingest_content("blob-fixture-stdin", bytes)
            .expect("content");
        let grant = ReadGrant::issue(
            b"content-token",
            "agent-fixture-001",
            Some("run-fixture-001".to_owned()),
            CapturePrivilege::Content,
            Duration::from_secs(60),
            NOW,
        )
        .expect("grant");
        let summary = store
            .component(
                b"content-token",
                &grant,
                "snapshot-fixture-cli-001",
                "component-fixture-stdin",
                NOW,
            )
            .expect("component");
        assert!(summary.content_available);
        assert_eq!(
            store
                .content(
                    b"content-token",
                    &grant,
                    "snapshot-fixture-cli-001",
                    "component-fixture-stdin",
                    NOW,
                )
                .expect("content"),
            bytes
        );
    }

    #[test]
    fn events_are_filtered_by_project_and_run_scope() {
        let mut store = Store::default();
        store.ingest(FIXTURE).expect("reader snapshot");
        store.ingest(CLI_FIXTURE).expect("fixture snapshot");
        store
            .append_event(
                &serde_json::to_vec(&serde_json::json!({
                    "schema":"ascension.context-event.v1",
                    "event_id":"event-hidden-sequence",
                    "producer_id":"producer-hidden",
                    "sequence":1,
                    "snapshot_id":"snapshot-t02-synthetic-001",
                    "provider_attempt_id":null,
                    "observed_at":"2026-09-09T00:00:00Z",
                    "event_type":"snapshot.prepared",
                    "details":{}
                }))
                .expect("hidden event"),
            )
            .expect("hidden event ingest");
        for line in include_bytes!("../../../fixtures/valid/events.jsonl")
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            store.append_event(line).expect("event");
        }
        let fixture_grant = ReadGrant::issue(
            b"fixture-token",
            "agent-fixture-001",
            Some("run-fixture-001".to_owned()),
            CapturePrivilege::Metadata,
            Duration::from_secs(60),
            NOW,
        )
        .expect("grant");
        assert_eq!(
            store
                .events(
                    b"fixture-token",
                    &fixture_grant,
                    "run-fixture-001",
                    None,
                    20,
                    NOW,
                )
                .expect("events")
                .events
                .len(),
            7
        );
        let first_page = store
            .events(
                b"fixture-token",
                &fixture_grant,
                "run-fixture-001",
                None,
                2,
                NOW,
            )
            .expect("first page");
        assert_eq!(
            first_page
                .events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(first_page.next_cursor, Some(1));
        assert!(!first_page.gap);
        let second_page = store
            .events(
                b"fixture-token",
                &fixture_grant,
                "run-fixture-001",
                first_page.next_cursor,
                2,
                NOW,
            )
            .expect("second page");
        assert_eq!(
            second_page
                .events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(!second_page.gap);
        assert!(
            store
                .events(
                    b"fixture-token",
                    &fixture_grant,
                    "run-t02-001",
                    None,
                    20,
                    NOW,
                )
                .expect("scoped empty page")
                .events
                .is_empty()
        );
    }

    #[test]
    fn event_cursor_remains_stable_when_a_late_producer_sequence_arrives() {
        let mut store = Store::default();
        store.ingest(CLI_FIXTURE).expect("fixture snapshot");
        let append = |store: &mut Store, event_id: &str, sequence: u64| {
            store
                .append_event(
                    &serde_json::to_vec(&serde_json::json!({
                        "schema":"ascension.context-event.v1",
                        "event_id":event_id,
                        "producer_id":"producer-ordered",
                        "sequence":sequence,
                        "snapshot_id":"snapshot-fixture-cli-001",
                        "provider_attempt_id":null,
                        "observed_at":"2026-09-09T00:00:00Z",
                        "event_type":"snapshot.prepared",
                        "details":{}
                    }))
                    .expect("event bytes"),
                )
                .expect("event");
        };
        append(&mut store, "event-ordered-0", 0);
        append(&mut store, "event-ordered-2", 2);
        let grant = ReadGrant::issue(
            b"fixture-token",
            "agent-fixture-001",
            Some("run-fixture-001".to_owned()),
            CapturePrivilege::Metadata,
            Duration::from_secs(60),
            NOW,
        )
        .expect("grant");
        let first_page = store
            .events(b"fixture-token", &grant, "run-fixture-001", None, 1, NOW)
            .expect("first page");
        assert_eq!(
            first_page
                .events
                .iter()
                .map(|event| event.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-ordered-0"]
        );
        assert_eq!(first_page.next_cursor, Some(0));

        // The producer's missing sequence arrives after the page was issued. Its immutable
        // scope ordinal follows the already visible event and cannot be skipped by cursor 0.
        append(&mut store, "event-ordered-1", 1);
        let second_page = store
            .events(
                b"fixture-token",
                &grant,
                "run-fixture-001",
                first_page.next_cursor,
                10,
                NOW,
            )
            .expect("second page");
        assert_eq!(
            second_page
                .events
                .iter()
                .map(|event| (event.event_id.as_str(), event.sequence))
                .collect::<Vec<_>>(),
            vec![("event-ordered-2", 1), ("event-ordered-1", 2)]
        );
    }

    #[test]
    fn out_of_scope_ids_have_the_same_not_found_result_as_unknown_ids() {
        let mut store = Store::default();
        store.ingest(FIXTURE).expect("fixture");
        let grant = grant(CapturePrivilege::Metadata);
        assert_eq!(
            store
                .get(b"unit-test-token", &grant, "snapshot-fixture-cli-001", NOW,)
                .expect_err("other project is hidden"),
            ReadError::NotFound
        );
        assert_eq!(
            store
                .get(b"unit-test-token", &grant, "missing-snapshot", NOW)
                .expect_err("missing snapshot"),
            ReadError::NotFound
        );
    }

    #[test]
    fn plaintext_store_rejects_private_snapshots_with_content_refs() {
        let private = String::from_utf8(CLI_FIXTURE.to_vec())
            .expect("fixture utf8")
            .replace(
                "\"capture_mode\": \"memory\"",
                "\"capture_mode\": \"private\"",
            );
        let mut store = Store::default();
        assert!(matches!(
            store.ingest(private.as_bytes()),
            Err(IngestError::PrivateContentUnsupported)
        ));
    }
}
