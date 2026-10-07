#[cfg(unix)]
#[test]
fn retained_nonblocking_lock_denies_second_owner_without_replacing_entry() {
    use super::process_lock::{LockError, acquire};
    use super::protected_files::PrivateRoot;
    use super::tests_files::Fixture;
    use std::os::unix::fs::MetadataExt;

    let fixture = Fixture::new();
    let root_path = fixture.private_dir("operator");
    let root = PrivateRoot::open(&root_path).expect("private root");
    let first = acquire(&root).expect("first lock");
    let lock_path = root_path.join("owner-service.lock");
    let before = std::fs::metadata(&lock_path).expect("lock metadata");
    assert_eq!(before.mode() & 0o7777, 0o600);
    assert_eq!(before.nlink(), 1);
    assert!(matches!(acquire(&root), Err(LockError::Busy)));
    let after = std::fs::metadata(&lock_path).expect("retained lock metadata");
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    drop(first);
    let second = acquire(&root).expect("lock released when owner exits");
    drop(second);
}

#[cfg(unix)]
#[test]
fn lock_refuses_symlink_or_unprotected_preexisting_entry() {
    use super::process_lock::acquire;
    use super::protected_files::PrivateRoot;
    use super::tests_files::{Fixture, write_private_at};
    use std::os::unix::fs::{PermissionsExt, symlink};

    let fixture = Fixture::new();
    let root_path = fixture.private_dir("operator");
    let root = PrivateRoot::open(&root_path).expect("private root");
    let target = write_private_at(fixture.path(), "target", b"");
    symlink(target, root_path.join("owner-service.lock")).expect("lock symlink");
    assert!(acquire(&root).is_err());

    std::fs::remove_file(root_path.join("owner-service.lock")).expect("remove test symlink");
    let bad_lock = write_private_at(&root_path, "owner-service.lock", b"");
    std::fs::set_permissions(&bad_lock, std::fs::Permissions::from_mode(0o640))
        .expect("relax lock mode");
    assert!(acquire(&root).is_err());
}

#[cfg(unix)]
#[test]
fn retained_lock_fails_closed_if_reserved_path_is_replaced() {
    use super::process_lock::acquire;
    use super::protected_files::PrivateRoot;
    use super::tests_files::{Fixture, write_private_at};

    let fixture = Fixture::new();
    let root_path = fixture.private_dir("operator");
    let root = PrivateRoot::open(&root_path).expect("private root");
    let lock = acquire(&root).expect("exclusive lock");
    std::fs::rename(
        root_path.join("owner-service.lock"),
        root_path.join("old-lock-entry"),
    )
    .expect("move path during replacement test");
    write_private_at(&root_path, "owner-service.lock", b"");
    assert!(lock.check_current().is_err());
}

#[cfg(unix)]
#[test]
fn retained_lock_recheck_does_not_recreate_a_missing_path() {
    use super::process_lock::acquire;
    use super::protected_files::PrivateRoot;
    use super::tests_files::Fixture;

    let fixture = Fixture::new();
    let root_path = fixture.private_dir("operator");
    let root = PrivateRoot::open(&root_path).expect("private root");
    let lock = acquire(&root).expect("exclusive lock");
    let lock_path = root_path.join("owner-service.lock");
    std::fs::remove_file(&lock_path).expect("remove reserved path while fd remains locked");

    assert_eq!(
        lock.check_current(),
        Err(super::process_lock::LockError::Unavailable)
    );
    assert!(!lock_path.exists());
}
