// SPDX-License-Identifier: MIT

use super::ReadApi;
use super::capabilities::capabilities;
use crate::http::{ApiError, HttpResponse, query_limit};
use std::time::SystemTime;

impl<'a> ReadApi<'a> {
    /// Dispatches the frozen `/v1/...` GET-only read surface. Non-GET methods and bodies are
    /// rejected in `handle_at`; every reachable branch here is a read projection.
    pub(super) fn route(
        &self,
        path: &str,
        query: &[(String, String)],
        now: SystemTime,
    ) -> Result<HttpResponse, ApiError> {
        if path.contains("..") || path.contains('\\') || path.contains('%') {
            return Err(ApiError::BadRequest);
        }
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if segments == ["v1", "capabilities"] {
            self.authorize(now)?;
            return Ok(HttpResponse::json(200, capabilities()));
        }
        if segments == ["v1", "runs"] {
            return self.handle_runs(query_limit(query)?, now);
        }
        if segments.len() == 4
            && segments[..3] == ["v1", "runs", segments[2]]
            && segments[3] == "snapshots"
        {
            return self.handle_run_snapshots(segments[2], query_limit(query)?, now);
        }
        if segments.len() == 5
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "snapshots"
        {
            return self.handle_snapshot(segments[2], segments[4], now);
        }
        if segments.len() == 8
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "snapshots"
            && segments[5] == "components"
            && segments[7] == "content"
        {
            return self.handle_component_content(segments[2], segments[4], segments[6], now);
        }
        if segments.len() == 7
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "snapshots"
            && segments[5] == "components"
        {
            return self.handle_component(segments[2], segments[4], segments[6], now);
        }
        if segments.len() == 4
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "events"
        {
            return self.handle_events(segments[2], query, query_limit(query)?, now);
        }
        if segments.len() == 4
            && segments[0] == "v1"
            && segments[1] == "runs"
            && segments[3] == "compare"
        {
            return self.handle_compare(segments[2], query, now);
        }
        Err(ApiError::NotFound)
    }
}
