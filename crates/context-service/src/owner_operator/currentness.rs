use crate::authenticated_ingress::{
    AuthenticatedIngress, AuthenticatedIngressError, AuthenticatedOwnerInvocation,
};
use crate::harness_context_owner_wire::ContextOwnerInvocationV2;
use crate::harness_facade::{HarnessFacadeConfig, RetentionPolicy, SecretDigest};
use crate::harness_owner_transport::{LiveCurrentnessError, LiveInvocationCurrentness};
use crate::http::HttpRequest;
use crate::owner_invocation_store::{AdmissionUse, TrustedInvocationAdmission};
use crate::protected_owner_credentials::ResolvedOwnerCredential;

use super::config::OperatorConfig;
use super::grants::GuardedGrantStore;
use super::principal::FilePrincipalVerifier;
use super::process_lock::OperatorProcessLock;
use super::slots::FileOwnerSlotResolver;

pub(super) struct Currentness<'a> {
    pub(super) ingress: &'a mut AuthenticatedIngress<FilePrincipalVerifier, GuardedGrantStore>,
    pub(super) resolver: &'a mut FileOwnerSlotResolver,
    pub(super) config: &'a OperatorConfig,
    pub(super) request: &'a HttpRequest,
    pub(super) admitted: &'a AuthenticatedOwnerInvocation,
    pub(super) journal_name: &'a str,
    pub(super) journal_identity: (u64, u64),
    pub(super) process_lock: &'a OperatorProcessLock,
}

impl Currentness<'_> {
    pub(super) fn check(
        &mut self,
        admission: &TrustedInvocationAdmission,
        invocation: &ContextOwnerInvocationV2,
        credential: &ResolvedOwnerCredential,
        use_kind: AdmissionUse,
        _now: u64,
    ) -> Result<(), LiveCurrentnessError> {
        self.process_lock
            .check_current()
            .map_err(|_| LiveCurrentnessError::Unavailable)?;
        if !std::ptr::eq(admission, self.admitted.admission())
            || invocation != self.admitted.invocation()
            || credential != self.admitted.credential()
            || use_kind != self.admitted.use_kind()
        {
            return Err(LiveCurrentnessError::Denied);
        }
        self.config
            .root
            .verify_current_root()
            .map_err(|_| LiveCurrentnessError::Unavailable)?;
        self.config
            .root
            .verify_database_files(self.journal_name, Some(self.journal_identity))
            .map_err(|_| LiveCurrentnessError::Unavailable)?;
        let csrf = self
            .config
            .root
            .read_file(&self.config.csrf_secret_ref, 4 * 1024)
            .map_err(|_| LiveCurrentnessError::Unavailable)?;
        let csrf_digest =
            SecretDigest::from_secret(&csrf).map_err(|_| LiveCurrentnessError::Unavailable)?;
        let facade = HarnessFacadeConfig::new(
            self.config.scope.clone(),
            self.config.expected_host.clone(),
            self.config.expected_origin.clone(),
            Some(csrf_digest),
            RetentionPolicy::default(),
        )
        .map_err(|_| LiveCurrentnessError::Unavailable)?;
        self.ingress
            .revalidate_current_owner_identity(self.request, &facade, self.admitted, self.resolver)
            .map_err(map_ingress_error)?;
        self.config
            .root
            .verify_current_root()
            .map_err(|_| LiveCurrentnessError::Unavailable)?;
        self.config
            .root
            .verify_database_files(self.journal_name, Some(self.journal_identity))
            .map_err(|_| LiveCurrentnessError::Unavailable)?;
        self.process_lock
            .check_current()
            .map_err(|_| LiveCurrentnessError::Unavailable)
    }
}

impl LiveInvocationCurrentness for Currentness<'_> {
    fn revalidate(
        &mut self,
        admission: &TrustedInvocationAdmission,
        invocation: &ContextOwnerInvocationV2,
        credential: &ResolvedOwnerCredential,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<(), LiveCurrentnessError> {
        self.check(admission, invocation, credential, use_kind, now)
    }
}

fn map_ingress_error(error: AuthenticatedIngressError) -> LiveCurrentnessError {
    match error {
        AuthenticatedIngressError::VerifierUnavailable
        | AuthenticatedIngressError::GrantStoreUnavailable
        | AuthenticatedIngressError::OwnerCredentialUnavailable => {
            LiveCurrentnessError::Unavailable
        }
        _ => LiveCurrentnessError::Denied,
    }
}
