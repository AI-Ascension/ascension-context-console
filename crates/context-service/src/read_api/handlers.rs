// SPDX-License-Identifier: MIT

use super::ReadApi;
use crate::http::{ApiError, HttpResponse, query_value};
use crate::store::{CapturePrivilege, ReadError};
use serde_json::{Value, json};
use std::time::SystemTime;

impl<'a> ReadApi<'a> {
    pub(super) fn handle_runs(
        &self,
        limit: usize,
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let summaries = self
            .store
            .list(self.token, self.grant, now, limit)
            .map_err(map_read_error)?;
        let mut runs = Vec::new();
        for summary in summaries {
            if !runs.iter().any(|run: &String| run == &summary.run_id) {
                runs.push(summary.run_id);
            }
        }
        Ok(HttpResponse::json(
            200,
            json!({"runs":runs,"next_cursor":null}),
        ))
    }

    pub(super) fn handle_run_snapshots(
        &self,
        run_id: &str,
        limit: usize,
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let summaries = self
            .store
            .list(self.token, self.grant, now, limit)
            .map_err(map_read_error)?
            .into_iter()
            .filter(|summary| summary.run_id == run_id)
            .collect::<Vec<_>>();
        Ok(HttpResponse::json(
            200,
            json!({"run_id":run_id,"snapshots":summaries,"next_cursor":null}),
        ))
    }

    pub(super) fn handle_snapshot(
        &self,
        run_id: &str,
        snapshot_id: &str,
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let bytes = self
            .store
            .get(self.token, self.grant, snapshot_id, now)
            .map_err(map_read_error)?;
        let snapshot: Value = serde_json::from_slice(&bytes).map_err(|_| ApiError::BadRequest)?;
        if snapshot
            .get("identity")
            .and_then(|value| value.get("run_id"))
            .and_then(Value::as_str)
            != Some(run_id)
        {
            return Err(ApiError::NotFound);
        }
        Ok(HttpResponse::raw_json(200, bytes))
    }

    pub(super) fn handle_component_content(
        &self,
        run_id: &str,
        snapshot_id: &str,
        component_id: &str,
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let summary = self
            .store
            .component(self.token, self.grant, snapshot_id, component_id, now)
            .map_err(map_read_error)?;
        if !self
            .store
            .list(self.token, self.grant, now, 200)
            .map_err(map_read_error)?
            .iter()
            .any(|item| item.snapshot_id == snapshot_id && item.run_id == run_id)
        {
            return Err(ApiError::NotFound);
        }
        let bytes = self
            .store
            .content(self.token, self.grant, snapshot_id, component_id, now)
            .map_err(map_read_error)?;
        Ok(HttpResponse::content(200, &summary.media_type, bytes))
    }

    pub(super) fn handle_component(
        &self,
        run_id: &str,
        snapshot_id: &str,
        component_id: &str,
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let summary = self
            .store
            .component(self.token, self.grant, snapshot_id, component_id, now)
            .map_err(map_read_error)?;
        if summary.snapshot_id.is_empty()
            || !self
                .store
                .list(self.token, self.grant, now, 200)
                .map_err(map_read_error)?
                .iter()
                .any(|item| item.snapshot_id == snapshot_id && item.run_id == run_id)
        {
            return Err(ApiError::NotFound);
        }
        let content = if self.grant.privilege() == CapturePrivilege::Content {
            "content_privilege_granted"
        } else {
            "content_not_authorized"
        };
        Ok(HttpResponse::json(
            200,
            json!({"component":summary,"content":content}),
        ))
    }

    pub(super) fn handle_events(
        &self,
        run_id: &str,
        query: &[(String, String)],
        limit: usize,
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let after = query
            .iter()
            .find(|(key, _)| key == "cursor")
            .map(|(_, value)| value.as_str());
        let page = self
            .store
            .events(self.token, self.grant, run_id, after, limit, now)
            .map_err(map_read_error)?;
        let events: Vec<Value> = page
            .events
            .iter()
            .map(|event| {
                json!({
                    "event_id":event.event_id,
                    "producer_id":event.producer_id,
                    "sequence":event.sequence,
                    "snapshot_id":event.snapshot_id,
                    "provider_attempt_id":event.provider_attempt_id,
                    "observed_at":event.observed_at,
                    "event_type":event.event_type.as_str(),
                    "details":event.details,
                })
            })
            .collect();
        Ok(HttpResponse::json(
            200,
            json!({"run_id":run_id,"events":events,"next_cursor":page.next_cursor,"gap":page.gap,"offline":false}),
        ))
    }

    pub(super) fn handle_compare(
        &self,
        run_id: &str,
        query: &[(String, String)],
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        let left = query_value(query, "left")?;
        let right = query_value(query, "right")?;
        let result = self
            .store
            .compare_for_run(self.token, self.grant, run_id, left, right, now)
            .map_err(map_read_error)?;
        Ok(HttpResponse::json(
            200,
            json!({"comparison":result,"read_only":true}),
        ))
    }
}

pub(super) fn map_read_error(error: ReadError) -> ApiError {
    match error {
        ReadError::InvalidToken | ReadError::Expired => ApiError::Unauthorized,
        ReadError::InvalidScope | ReadError::Forbidden => ApiError::Forbidden,
        ReadError::NotFound => ApiError::NotFound,
        ReadError::TooLarge => ApiError::TooLarge,
    }
}
