// SPDX-License-Identifier: MIT

use super::{parse_event, parse_event_lines};

const EVENTS: &[u8] = include_bytes!("../../../../fixtures/valid/events.jsonl");

#[test]
fn parses_fixture_event_lines() {
    let result = parse_event_lines(EVENTS);
    assert!(result.is_ok());
    if let Ok(events) = result {
        assert_eq!(events.len(), 7);
    }
}

#[test]
fn rejects_empty_object_event() {
    assert!(parse_event(b"{}").is_err());
}
