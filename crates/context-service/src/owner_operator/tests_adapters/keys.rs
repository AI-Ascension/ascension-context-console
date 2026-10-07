use super::super::keys::FileJournalKeyProvider;
use super::*;
use crate::owner_invocation_store::{OwnerInvocationKeyProvider, StoreError};

#[test]
fn journal_key_provider_loads_current_and_all_retained_keys_and_fails_closed() {
    let fixture = Fixture::new();
    let (config, state) = setup_config(&fixture);
    install_slot_files(&state, OWNER_REF, false);
    install_key_material(&state);
    let entries = json!([
        {"key_id":"data-v1","file_ref":"data-old.bin"},
        {"key_id":"data-v2","file_ref":"data-current.bin"}
    ]);
    write_private_at(&state, "journal-keys.json", &key_manifest(entries.clone()));
    let mut provider = FileJournalKeyProvider::new(&config);
    assert!(provider.load().is_ok());

    replace_private(
        &state.join("journal-keys.json"),
        "duplicate-ref.next",
        &key_manifest(json!([
            {"key_id":"data-v1","file_ref":"index.bin"},
            {"key_id":"data-v2","file_ref":"data-current.bin"}
        ])),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    replace_private(
        &state.join("journal-keys.json"),
        "cross-type-ref.next",
        &key_manifest(json!([
            {"key_id":"data-v1","file_ref":"principal-mac.bin"},
            {"key_id":"data-v2","file_ref":"data-current.bin"}
        ])),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    replace_private(
        &state.join("journal-keys.json"),
        "database-sidecar.next",
        &key_manifest(json!([
            {"key_id":"data-v1","file_ref":"journal.sqlite3-wal"},
            {"key_id":"data-v2","file_ref":"data-current.bin"}
        ])),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    replace_private(
        &state.join("journal-keys.json"),
        "bearer-alias.next",
        &key_manifest(json!([
            {"key_id":"data-v1","file_ref":"harness-bearer.bin"},
            {"key_id":"data-v2","file_ref":"data-current.bin"}
        ])),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    replace_private(
        &state.join("journal-keys.json"),
        "duplicate-id.next",
        &key_manifest(json!([
            {"key_id":"data-v2","file_ref":"data-old.bin"},
            {"key_id":"data-v2","file_ref":"data-current.bin"}
        ])),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    replace_private(
        &state.join("journal-keys.json"),
        "too-many.next",
        &key_manifest(
            (0..9)
                .map(|index| {
                    json!({
                        "key_id": format!("data-{index}"),
                        "file_ref": format!("key-{index}.bin")
                    })
                })
                .collect::<Vec<_>>(),
        ),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    let mut duplicate_field = String::from_utf8(key_manifest(entries)).unwrap();
    duplicate_field = duplicate_field.replacen(
        "\"schema_version\":",
        "\"schema_version\":\"ascension.context-console.owner-journal-keys.v1\",\"schema_version\":",
        1,
    );
    replace_private(
        &state.join("journal-keys.json"),
        "duplicate-field.next",
        duplicate_field.as_bytes(),
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));

    let mut unknown_field = key_manifest(json!([
        {"key_id":"data-v1","file_ref":"data-old.bin"},
        {"key_id":"data-v2","file_ref":"data-current.bin"}
    ]));
    unknown_field.pop();
    unknown_field.extend_from_slice(b",\"unknown\":true}");
    replace_private(
        &state.join("journal-keys.json"),
        "unknown-field.next",
        &unknown_field,
    );
    assert_eq!(provider.load().err(), Some(StoreError::KeyUnavailable));
}

#[test]
fn key_material_constructor_rejects_reused_index_and_data_bytes() {
    let repeated = [0x35; 32];
    let mut keys = std::collections::BTreeMap::new();
    keys.insert("data-v1".to_owned(), repeated);
    assert_eq!(
        crate::owner_invocation_store::OwnerInvocationKeyMaterial::new(
            "index-v1".to_owned(),
            repeated,
            "data-v1".to_owned(),
            keys,
        )
        .err(),
        Some(StoreError::KeyUnavailable)
    );

    let mut distinct_ids = std::collections::BTreeMap::new();
    distinct_ids.insert("data-v1".to_owned(), [0x45; 32]);
    distinct_ids.insert("data-v2".to_owned(), [0x45; 32]);
    assert!(
        crate::owner_invocation_store::OwnerInvocationKeyMaterial::new(
            "index-v1".to_owned(),
            [0x36; 32],
            "data-v2".to_owned(),
            distinct_ids,
        )
        .is_err()
    );
}
