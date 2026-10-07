use super::{
    AdmittedSubjectGrant, SqliteSubjectGrantStore, SubjectGrantError, SubjectGrantSpec,
    SubjectGrantStore, VerifiedPrincipal,
};
use crate::control::Scope;
use crate::harness_facade::FacadePermission;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub(crate) struct OperatorSubjectGrantStore {
    inner: SqliteSubjectGrantStore,
    path: PathBuf,
    identity: (u64, u64),
}

impl OperatorSubjectGrantStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self, SubjectGrantError> {
        if !cfg!(unix) {
            return Err(SubjectGrantError::StoreUnavailable);
        }
        let path = super::storage::checked_database_path(path.as_ref())?;
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| SubjectGrantError::StoreUnavailable)?;
        let inner = SqliteSubjectGrantStore::initialize(connection, path.clone())?;
        let identity = file_identity(&path)?;
        let store = Self {
            inner,
            path,
            identity,
        };
        store.check_files()?;
        Ok(store)
    }

    pub(crate) fn provision(&mut self, spec: &SubjectGrantSpec) -> Result<(), SubjectGrantError> {
        self.check_files()?;
        let result = self.inner.provision(spec);
        self.check_files()?;
        result
    }

    pub(crate) fn revoke(&mut self, grant_id: &str) -> Result<u64, SubjectGrantError> {
        self.check_files()?;
        let result = self.inner.revoke(grant_id);
        self.check_files()?;
        result
    }

    fn check_files(&self) -> Result<(), SubjectGrantError> {
        if file_identity(&self.path)? != self.identity {
            return Err(SubjectGrantError::StoreUnavailable);
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            verify_optional_private_file(&with_suffix(&self.path, suffix))?;
        }
        Ok(())
    }
}

impl SubjectGrantStore for OperatorSubjectGrantStore {
    fn reserve(
        &mut self,
        principal: &VerifiedPrincipal,
        scope: &Scope,
        required: &[FacadePermission],
        optional: &[FacadePermission],
        now: u64,
    ) -> Result<Vec<AdmittedSubjectGrant>, SubjectGrantError> {
        self.check_files()?;
        let result = self
            .inner
            .reserve(principal, scope, required, optional, now);
        self.check_files()?;
        result
    }
}

#[cfg(unix)]
fn file_identity(path: &Path) -> Result<(u64, u64), SubjectGrantError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SubjectGrantError::StoreUnavailable)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
        || metadata.permissions().mode() & 0o7777 != 0o600
    {
        return Err(SubjectGrantError::StoreUnavailable);
    }
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(_path: &Path) -> Result<(u64, u64), SubjectGrantError> {
    Err(SubjectGrantError::StoreUnavailable)
}

fn verify_optional_private_file(path: &Path) -> Result<(), SubjectGrantError> {
    match file_identity(path) {
        Ok(_) => Ok(()),
        Err(SubjectGrantError::StoreUnavailable)
            if std::fs::symlink_metadata(path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
