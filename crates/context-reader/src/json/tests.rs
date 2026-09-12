// SPDX-License-Identifier: MIT

use super::parse;

#[test]
fn parses_valid_object() {
    let result = parse(br#"{"a":1,"b":[true,null]}"#);
    assert!(result.is_ok());
    if let Ok(value) = result {
        assert!(value.object().is_some());
    }
}

#[test]
fn duplicate_key_is_rejected() {
    assert!(parse(br#"{"a":1,"a":2}"#).is_err());
}

#[test]
fn trailing_bytes_are_rejected() {
    assert!(parse(b"{} x").is_err());
}
