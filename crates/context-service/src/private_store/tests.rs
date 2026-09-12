// SPDX-License-Identifier: MIT

use super::*;

fn vault() -> PrivateVault {
    PrivateVault::new(
        [7_u8; 32],
        1024,
        PolicyApproval {
            accepted: true,
            restricted_authorization: true,
        },
    )
    .expect("vault")
}

fn scope(content_ref: &str) -> PrivateScope {
    PrivateScope::new(
        "agent-private",
        "snapshot-private",
        "component-private",
        content_ref,
    )
    .expect("scope")
}

#[test]
fn policy_never_downgrades_to_plaintext() {
    assert!(matches!(
        PrivateVault::new(
            [7_u8; 32],
            1024,
            PolicyApproval {
                accepted: false,
                restricted_authorization: true,
            }
        ),
        Err(PrivateStoreError::PolicyNotApproved)
    ));
}

#[test]
fn encrypted_content_round_trips_and_tampering_fails() {
    let mut vault = vault();
    let scope = scope("blob-1");
    vault.put(&scope, b"private synthetic marker").expect("put");
    assert_ne!(
        vault.ciphertext(&scope).expect("ciphertext"),
        b"private synthetic marker"
    );
    assert_eq!(
        vault.get(&scope, true).expect("get"),
        b"private synthetic marker"
    );
    assert!(matches!(
        vault.get(&scope, false),
        Err(PrivateStoreError::Unauthorized)
    ));
    let object = vault.objects.get_mut(&scope.storage_key()).expect("object");
    object.ciphertext[0] ^= 1;
    assert!(matches!(
        vault.get(&scope, true),
        Err(PrivateStoreError::AuthenticationFailed)
    ));
}

#[test]
fn ciphertext_is_bound_to_its_content_reference() {
    let mut vault = vault();
    let first = scope("blob-1");
    let second = scope("blob-2");
    vault.put(&first, b"private synthetic marker").expect("put");
    let object = vault.objects.remove(&first.storage_key()).expect("object");
    vault.objects.insert(second.storage_key(), object);
    assert!(matches!(
        vault.get(&second, true),
        Err(PrivateStoreError::AuthenticationFailed)
    ));
}

#[test]
fn private_scope_binds_project_snapshot_and_component_identity() {
    let mut vault = vault();
    let original = scope("blob-1");
    vault
        .put(&original, b"private synthetic marker")
        .expect("put");
    let moved = PrivateScope::new(
        "other-project",
        original.snapshot_id(),
        original.component_id(),
        original.content_ref(),
    )
    .expect("scope");
    assert!(matches!(
        vault.get(&moved, true),
        Err(PrivateStoreError::NotFound)
    ));
}
