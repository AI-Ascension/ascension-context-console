#[cfg(unix)]
mod unix {

    use super::super::protected_files::{
        AdapterError, PrivateRoot, open_absolute, read_open_file, read_private_file,
        verify_file_attributes,
    };
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    pub(in crate::owner_operator) struct Fixture {
        directory: PathBuf,
    }

    impl Fixture {
        pub(in crate::owner_operator) fn new() -> Self {
            let target = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
            let target = if target.is_absolute() {
                target
            } else {
                std::env::current_dir()
                    .expect("current directory")
                    .join(target)
            };
            let fixtures = target.join("console18-operator-fixtures");
            fs::create_dir_all(&fixtures).expect("create designated test fixture directory");
            fs::set_permissions(&fixtures, fs::Permissions::from_mode(0o700))
                .expect("secure test fixture directory");
            loop {
                let suffix = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
                let directory = fixtures.join(format!("{}-{suffix}", std::process::id()));
                match fs::create_dir(&directory) {
                    Ok(()) => {
                        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                            .expect("secure unique fixture directory");
                        return Self { directory };
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create fixture: {error}"),
                }
            }
        }

        pub(in crate::owner_operator) fn path(&self) -> &Path {
            &self.directory
        }

        pub(in crate::owner_operator) fn private_dir(&self, name: &str) -> PathBuf {
            let path = self.directory.join(name);
            fs::create_dir(&path).expect("create private test directory");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("secure private test directory");
            path
        }

        pub(in crate::owner_operator) fn write_private(
            &self,
            name: &str,
            contents: &[u8],
        ) -> PathBuf {
            write_private_at(&self.directory, name, contents)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    pub(in crate::owner_operator) fn write_private_at(
        directory: &Path,
        name: &str,
        contents: &[u8],
    ) -> PathBuf {
        let path = directory.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .expect("create private file");
        file.write_all(contents).expect("write private fixture");
        file.sync_all().expect("sync private fixture");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .expect("set private file mode");
        path
    }

    #[test]
    fn descriptor_reader_enforces_bounds_and_reads_from_the_open_file() {
        let fixture = Fixture::new();
        let private = fixture.private_dir("private");
        let path = write_private_at(&private, "config.json", b"closed");
        assert_eq!(
            read_private_file(&path, 6)
                .expect("bounded read")
                .as_slice(),
            b"closed"
        );
        assert!(matches!(
            read_private_file(&path, 5),
            Err(AdapterError::Invalid)
        ));

        let opened = open_absolute(&path, true).expect("open stable source fd");
        fs::rename(&path, private.join("old.json")).expect("rename original source");
        write_private_at(&private, "config.json", b"replaced");
        assert_eq!(
            read_open_file(opened, 16)
                .expect("read opened fd")
                .as_slice(),
            b"closed"
        );
    }

    #[test]
    fn descriptor_walk_rejects_symlinks_nonregular_files_and_unsafe_config_parent() {
        let fixture = Fixture::new();
        let private = fixture.private_dir("private");
        let file = write_private_at(&private, "plain", b"safe");
        let final_link = private.join("final-link");
        symlink(&file, &final_link).expect("create final symlink");
        assert!(read_private_file(&final_link, 16).is_err());

        let ancestor_link = fixture.path().join("ancestor-link");
        symlink(&private, &ancestor_link).expect("create ancestor symlink");
        assert!(read_private_file(&ancestor_link.join("plain"), 16).is_err());

        let directory = private.join("directory");
        fs::create_dir(&directory).expect("create nonregular target");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .expect("secure nonregular target");
        let root = PrivateRoot::open(&private).expect("open private root");
        assert!(matches!(
            root.read_file("directory", 16),
            Err(AdapterError::Invalid)
        ));

        fs::set_permissions(&private, fs::Permissions::from_mode(0o720))
            .expect("make immediate config parent unsafe");
        assert!(read_private_file(&file, 16).is_err());
    }

    #[test]
    fn private_root_descriptor_stays_anchored_across_path_replacement() {
        let fixture = Fixture::new();
        let root_path = fixture.private_dir("root");
        write_private_at(&root_path, "registry", b"old");
        let root = PrivateRoot::open(&root_path).expect("open anchored root");
        assert_eq!(root.verify_current_root(), Ok(()));

        let moved = fixture.path().join("root-old");
        fs::rename(&root_path, &moved).expect("move opened directory");
        fs::create_dir(&root_path).expect("replace directory name");
        fs::set_permissions(&root_path, fs::Permissions::from_mode(0o700))
            .expect("secure replacement directory");
        write_private_at(&root_path, "registry", b"new");

        assert!(root.verify_current_root().is_err());
        assert!(root.read_file("registry", 8).is_err());
    }

    #[test]
    fn database_helper_rejects_replacement_and_unsafe_sidecar() {
        let fixture = Fixture::new();
        let root_path = fixture.private_dir("state");
        let root = PrivateRoot::open(&root_path).expect("open private state root");
        let identity = root
            .ensure_database_file("journal.sqlite3")
            .expect("create protected database file");
        assert_eq!(
            root.verify_database_files("journal.sqlite3", Some(identity)),
            Ok(identity)
        );

        let database = root_path.join("journal.sqlite3");
        fs::rename(&database, root_path.join("old-journal.sqlite3"))
            .expect("move original database entry");
        write_private_at(&root_path, "journal.sqlite3", b"replacement");
        assert!(
            root.verify_database_files("journal.sqlite3", Some(identity))
                .is_err()
        );

        let other_identity = root
            .ensure_database_file("grants.sqlite3")
            .expect("create second protected database");
        let target = write_private_at(&root_path, "sidecar-target", b"private");
        symlink(&target, root_path.join("grants.sqlite3-wal")).expect("create sidecar symlink");
        assert!(
            root.verify_database_files("grants.sqlite3", Some(other_identity))
                .is_err()
        );
    }

    #[test]
    fn private_file_attributes_reject_wrong_uid_mode_and_link_count() {
        assert_eq!(verify_file_attributes(true, 9, 1, 0o600, 9), Ok(()));
        assert_eq!(
            verify_file_attributes(false, 9, 1, 0o600, 9),
            Err(AdapterError::Invalid)
        );
        assert_eq!(
            verify_file_attributes(true, 10, 1, 0o600, 9),
            Err(AdapterError::Invalid)
        );
        assert_eq!(
            verify_file_attributes(true, 9, 2, 0o600, 9),
            Err(AdapterError::Invalid)
        );
        assert_eq!(
            verify_file_attributes(true, 9, 1, 0o640, 9),
            Err(AdapterError::Invalid)
        );
        assert_eq!(
            verify_file_attributes(true, 9, 1, 0o1600, 9),
            Err(AdapterError::Invalid)
        );
    }
}

#[cfg(unix)]
pub(in crate::owner_operator) use unix::{Fixture, write_private_at};

#[cfg(not(unix))]
#[test]
fn private_file_adapters_refuse_non_unix_before_loading() {
    assert!(matches!(
        super::protected_files::PrivateRoot::open(std::path::Path::new("config")),
        Err(super::protected_files::AdapterError::UnsupportedPlatform)
    ));
}
