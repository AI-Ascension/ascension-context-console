// SPDX-License-Identifier: MIT

// `#[serde(skip)]` governs serialization only. These assertions pin that `ControlPlane` no longer
// prints its skipped item content or prepared input material through `{:?}` or `{:#?}`, while still
// emitting positive controls so the test cannot pass vacuously. See issue #33.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use super::*;

const MARKER: &str = "CONSOLE-ITEM-CONTENT-MARKER";

// A leaked `Vec<u8>` renders as a numeric array; the ASCII marker alone would not detect it.
fn leaked_byte_array() -> String {
    format!("{:?}", MARKER.as_bytes())
}

fn assert_redacted(value: &ControlPlane) {
    for rendered in [format!("{value:?}"), format!("{value:#?}")] {
        for control in ["ControlPlane", "item_count", "prepared_input_len"] {
            assert!(
                rendered.contains(control),
                "missing positive control {control:?} in {rendered}"
            );
        }
        for forbidden in [MARKER, leaked_byte_array().as_str()] {
            assert!(
                !rendered.contains(forbidden),
                "ControlPlane formatting leaked {forbidden:?} in {rendered}"
            );
        }
    }
}

#[test]
fn control_plane_debug_withholds_item_content_and_prepared_input() {
    let mut plane = ControlPlane::synthetic();
    plane.items.insert(
        ("item-1".to_owned(), 1),
        ItemRecord {
            item: item_ref("item-1", 1, MARKER.as_bytes()),
            kind: "note".to_owned(),
            protected: true,
            scope: plane.scope.clone(),
            content: MARKER.as_bytes().to_vec(),
            expires_at: 0,
            expires_text: "never".to_owned(),
            locked_reason: None,
        },
    );
    plane.prepared_input = Some(MARKER.as_bytes().to_vec());
    assert_redacted(&plane);
}

#[test]
fn serialization_still_omits_the_skipped_items() {
    let mut plane = ControlPlane::synthetic();
    plane.items.insert(
        ("item-1".to_owned(), 1),
        ItemRecord {
            item: item_ref("item-1", 1, MARKER.as_bytes()),
            kind: "note".to_owned(),
            protected: true,
            scope: plane.scope.clone(),
            content: MARKER.as_bytes().to_vec(),
            expires_at: 0,
            expires_text: "never".to_owned(),
            locked_reason: None,
        },
    );
    let json = serde_json::to_string(&plane).expect("serialize");
    assert!(
        !json.contains(MARKER),
        "serialization leaked marker: {json}"
    );
}
