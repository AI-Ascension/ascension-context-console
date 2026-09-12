// SPDX-License-Identifier: MIT

use context_reader::{CaptureEvent, parse_event};
use sha2::{Digest, Sha256};
use std::time::SystemTime;

use super::cursor::{decode_event_cursor, encode_event_cursor};
use super::error::{IngestError, ReadError};
use super::grant::{ReadGrant, valid_id};
use super::summary::EventPage;
use super::{MAX_EVENTS, Store, StoredEvent};

impl Store {
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

    pub fn events(
        &self,
        token: &[u8],
        grant: &ReadGrant,
        run_id: &str,
        after_cursor: Option<&str>,
        limit: usize,
        now: SystemTime,
    ) -> Result<EventPage, ReadError> {
        if !valid_id(run_id) || limit == 0 || limit > 200 {
            return Err(ReadError::TooLarge);
        }
        self.authorize(token, grant, now)?;
        if grant.run().is_some_and(|scoped_run| scoped_run != run_id) {
            // Keep run-scoped grants indistinguishable from an unknown run.
            return Err(ReadError::NotFound);
        }
        let after_sequence = after_cursor
            .map(|cursor| decode_event_cursor(grant, run_id, cursor))
            .transpose()?;
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
            Some(encode_event_cursor(grant, run_id, next))
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
}
