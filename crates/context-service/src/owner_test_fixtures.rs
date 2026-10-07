use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct PrivateTestDirectory {
    path: PathBuf,
}

impl PrivateTestDirectory {
    pub(crate) fn new(label: &str) -> Self {
        assert!(
            !label.is_empty() && label.bytes().all(|byte| byte.is_ascii_alphanumeric()),
            "fixture label"
        );
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
        let fixtures = target.join("console18-owner-test-fixtures");
        fs::create_dir_all(&fixtures).expect("create designated fixture directory");
        secure_directory(&fixtures);
        loop {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = fixtures.join(format!("{label}-{}-{sequence}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    secure_directory(&path);
                    return Self { path };
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create fixture directory: {error}"),
            }
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateTestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn secure_directory(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .expect("secure test fixture directory");
    }
    #[cfg(not(unix))]
    let _ = path;
}
