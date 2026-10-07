use crate::authenticated_ingress::VerifiedPrincipal;
use crate::control::Scope;
use crate::harness_facade::FacadePermission;
use crate::subject_grants::{
    AdmittedSubjectGrant, OperatorSubjectGrantStore, SubjectGrantError, SubjectGrantSpec,
    SubjectGrantStore,
};

use super::config::OperatorConfig;
use super::protected_files::{AdapterError, PrivateRoot};

pub(super) struct GuardedGrantStore {
    inner: OperatorSubjectGrantStore,
    root: PrivateRoot,
    database_name: String,
    database_identity: (u64, u64),
}

impl GuardedGrantStore {
    pub(super) fn open(config: &OperatorConfig) -> Result<Self, SubjectGrantError> {
        let database_name = config
            .grant_database
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or(SubjectGrantError::Invalid)?
            .to_owned();
        let database_identity = config
            .root
            .ensure_database_file(&database_name)
            .map_err(map_file_error)?;
        let database_path = config
            .root
            .database_path(&database_name)
            .map_err(map_file_error)?;
        let inner = OperatorSubjectGrantStore::open(database_path)?;
        let store = Self {
            inner,
            root: config.root.clone(),
            database_name,
            database_identity,
        };
        store.check_files()?;
        Ok(store)
    }

    pub(super) fn provision(&mut self, spec: &SubjectGrantSpec) -> Result<(), SubjectGrantError> {
        self.check_files()?;
        let result = self.inner.provision(spec);
        self.check_files()?;
        result
    }

    pub(super) fn revoke(&mut self, grant_id: &str) -> Result<u64, SubjectGrantError> {
        self.check_files()?;
        let result = self.inner.revoke(grant_id);
        self.check_files()?;
        result
    }

    fn check_files(&self) -> Result<(), SubjectGrantError> {
        self.root
            .verify_database_files(&self.database_name, Some(self.database_identity))
            .map(|_| ())
            .map_err(map_file_error)
    }
}

impl SubjectGrantStore for GuardedGrantStore {
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

fn map_file_error(error: AdapterError) -> SubjectGrantError {
    match error {
        AdapterError::Invalid | AdapterError::Denied => SubjectGrantError::Invalid,
        #[cfg(not(unix))]
        AdapterError::UnsupportedPlatform => SubjectGrantError::StoreUnavailable,
        AdapterError::Unavailable | AdapterError::KeyUnavailable => {
            SubjectGrantError::StoreUnavailable
        }
    }
}
