//! Private, typed, one-request transport for the frozen Harness context-owner v2 boundary.
//!
//! Callers must first complete current authenticated-ingress admission. Writes and exact
//! lookups are accepted only with non-forgeable store permits, returned after their durable claim
//! transaction commits. This module has no raw URL, method, body, header, or bearer-send API.

use crate::harness_context_owner_wire::{
    ContextOwnerEndpointV1, ContextOwnerInvocationV2, HarnessHttpMethod, HarnessResponseV1,
    MAX_HARNESS_JSON_BODY_BYTES,
};
use crate::harness_facade::ProtectedAuthReference;
use crate::owner_invocation_store::{
    AdmissionUse, TrustedInvocationAdmission, required_harness_scopes,
};
use crate::protected_owner_credentials::{OwnerCredentialDescriptor, ResolvedOwnerCredential};
use std::net::SocketAddr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

#[path = "harness_owner_transport/authorization.rs"]
mod authorization;
#[path = "harness_owner_transport/errors.rs"]
mod errors;
#[path = "harness_owner_transport/framing.rs"]
mod framing;
#[path = "harness_owner_transport/journal.rs"]
mod journal;

use authorization::{validate_bearer, validate_bearer_binding, validate_closed_endpoint};
use errors::management_error;
pub(crate) use errors::{
    CredentialRedemptionError, HarnessTransportError, SanitizedManagementClass,
};

#[cfg(test)]
#[path = "harness_owner_transport/tests.rs"]
mod tests;
#[cfg(all(test, unix))]
#[path = "harness_owner_transport/tests_currentness.rs"]
mod tests_currentness;

const MAX_TRANSPORT_DEADLINE: Duration = Duration::from_secs(5);
const MAX_BEARER_BYTES: usize = 4 * 1024;
const MAX_PATH_BYTES: usize = 1024;
const MAX_QUERY_ITEMS: usize = 8;

/// Fixed, numeric loopback authority. The request DTO cannot influence this address.
pub(crate) struct HarnessOwnerTransportConfig {
    address: SocketAddr,
    deadline: Duration,
}

impl HarnessOwnerTransportConfig {
    pub(crate) fn new(
        address: SocketAddr,
        deadline: Duration,
    ) -> Result<Self, HarnessTransportError> {
        if !address.ip().is_loopback()
            || address.port() == 0
            || deadline.is_zero()
            || deadline > MAX_TRANSPORT_DEADLINE
        {
            return Err(HarnessTransportError::InvalidConfiguration);
        }
        Ok(Self { address, deadline })
    }
}

/// Trusted operator adapter that reloads one exact protected slot for every invocation.
/// Implementations must return only the current bearer for `reference` and `descriptor`; the
/// opaque reference and invocation claims are never accepted as bearer credentials.
pub(crate) trait HarnessCredentialRedeemer {
    fn redeem(
        &mut self,
        reference: &ProtectedAuthReference,
        descriptor: &OwnerCredentialDescriptor,
        invocation: &ContextOwnerInvocationV2,
        required_scopes: &[&'static str],
        now: u64,
    ) -> Result<Zeroizing<Vec<u8>>, CredentialRedemptionError>;
}

/// Required live check of ingress, grants, and protected credential against the immutable pair.
/// It runs after redemption before claim/network, after frame assembly immediately before connect,
/// and before disclosure; the final write/lookup check follows durable completion. It cannot
/// return a replacement invocation, credential, admission, or deadline.
pub(crate) trait LiveInvocationCurrentness {
    fn revalidate(
        &mut self,
        admission: &TrustedInvocationAdmission,
        invocation: &ContextOwnerInvocationV2,
        credential: &ResolvedOwnerCredential,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<(), LiveCurrentnessError>;
}

/// Closed callback result so transport errors never expose verifier, grant, or credential detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LiveCurrentnessError {
    Unavailable,
    Denied,
}

/// Ephemeral bearer bound to one exact typed invocation and its current protected reference.
/// It intentionally has no `Debug`, `Clone`, serialization, or public constructor.
#[must_use]
struct OneUseHarnessBearer {
    reference: ProtectedAuthReference,
    invocation: ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    required_scopes: Vec<&'static str>,
    expires_at: u64,
    token: Zeroizing<Vec<u8>>,
}

pub(crate) struct HarnessOwnerTransport {
    config: HarnessOwnerTransportConfig,
}

impl HarnessOwnerTransport {
    pub(crate) fn new(config: HarnessOwnerTransportConfig) -> Self {
        Self { config }
    }

    /// Redeem a fresh protected credential only after ingress has admitted the invocation.
    /// The returned bearer cannot be reused for another invocation or credential reference.
    fn redeem_for_invocation<R: HarnessCredentialRedeemer>(
        &self,
        invocation: &ContextOwnerInvocationV2,
        credential: &ResolvedOwnerCredential,
        redeemer: &mut R,
        use_kind: AdmissionUse,
        now: u64,
    ) -> Result<OneUseHarnessBearer, HarnessTransportError> {
        invocation
            .validate()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?;
        let endpoint = invocation
            .endpoint()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?;
        validate_closed_endpoint(invocation, &endpoint)?;
        let scopes = required_harness_scopes(&invocation.operation, use_kind)
            .map_err(|_| HarnessTransportError::UnsupportedOperation)?;
        let descriptor = credential.descriptor();
        let scope = descriptor.scope();
        if !descriptor.is_valid()
            || descriptor.expires_at() <= now
            || descriptor.console_issuer() != invocation.identity.console.issuer
            || descriptor.console_subject() != invocation.identity.console.subject
            || descriptor.harness_subject() != invocation.identity.harness.actor_subject
            || scope.project_id != invocation.identity.console_scope.project_id
            || scope.run_id != invocation.identity.console_scope.run_id
            || scope.episode_id != invocation.identity.console_scope.episode_id
            || scope.agent_id != invocation.identity.console_scope.agent_id
            || descriptor.expires_at() != invocation.identity.harness.credential_expires_at
            || credential.reference().as_str()
                != invocation.identity.harness.credential_reference_id
            || scopes
                .iter()
                .any(|required| !descriptor.permits_exact_scope(required))
        {
            return Err(HarnessTransportError::CredentialDenied);
        }
        let token = redeemer
            .redeem(credential.reference(), descriptor, invocation, &scopes, now)
            .map_err(|error| match error {
                CredentialRedemptionError::Unavailable => {
                    HarnessTransportError::CredentialUnavailable
                }
                CredentialRedemptionError::Denied => HarnessTransportError::CredentialDenied,
            })?;
        validate_bearer(&token)?;
        Ok(OneUseHarnessBearer {
            reference: credential.reference().clone(),
            invocation: invocation.clone(),
            use_kind,
            required_scopes: scopes,
            expires_at: descriptor.expires_at(),
            token,
        })
    }

    /// Send only a typed GET after the required live-currentness callback checks the admitted
    /// Console principal/grants and original protected credential.
    pub(crate) fn send_read_once<R: HarnessCredentialRedeemer>(
        &self,
        invocation: &ContextOwnerInvocationV2,
        admission: &TrustedInvocationAdmission,
        credential: &ResolvedOwnerCredential,
        redeemer: &mut R,
        currentness: &mut dyn LiveInvocationCurrentness,
        request_deadline: Instant,
    ) -> Result<HarnessResponseV1, HarnessTransportError> {
        let request_deadline = framing::operation_deadline(self.config.deadline, request_deadline)?;
        ensure_request_live(request_deadline)?;
        let now = trusted_unix_seconds()?;
        admission
            .validate_for(invocation, AdmissionUse::ReadOnly, now)
            .map_err(|_| HarnessTransportError::CredentialDenied)?;
        let endpoint = invocation
            .endpoint()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?;
        let body = invocation
            .harness_body()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?;
        if endpoint.method != HarnessHttpMethod::Get || body.is_some() {
            return Err(HarnessTransportError::UnsupportedOperation);
        }
        ensure_request_live(request_deadline)?;
        let bearer = self.redeem_for_invocation(
            invocation,
            credential,
            redeemer,
            AdmissionUse::ReadOnly,
            trusted_unix_seconds()?,
        )?;
        let now = trusted_unix_seconds()?;
        admission
            .validate_for(invocation, AdmissionUse::ReadOnly, now)
            .map_err(|_| HarnessTransportError::CredentialDenied)?;
        validate_bearer_binding(
            &bearer,
            invocation,
            AdmissionUse::ReadOnly,
            credential.reference(),
            now,
        )?;
        revalidate_currentness(
            currentness,
            admission,
            invocation,
            credential,
            AdmissionUse::ReadOnly,
            trusted_unix_seconds()?,
        )?;
        ensure_request_live(request_deadline)?;
        let effective_deadline = admitted_deadline(
            admission,
            invocation,
            AdmissionUse::ReadOnly,
            now,
            request_deadline,
        )?;
        ensure_request_live(effective_deadline)?;
        let reply = framing::exchange_with_authorization(
            &self.config,
            &endpoint,
            None,
            &bearer.token,
            effective_deadline,
            || {
                authorize_connection(
                    admission,
                    invocation,
                    AdmissionUse::ReadOnly,
                    &bearer,
                    credential.reference(),
                    credential,
                    currentness,
                    request_deadline,
                )
            },
        )?;
        let response_at = trusted_unix_seconds()?;
        ensure_request_live(effective_deadline)?;
        validate_live_admission(
            admission,
            invocation,
            AdmissionUse::ReadOnly,
            &bearer,
            credential.reference(),
            response_at,
        )?;
        if reply.status != 200 {
            let error = management_error(reply.status, &reply.body, &invocation.operation);
            validate_live_authority(
                admission,
                invocation,
                AdmissionUse::ReadOnly,
                &bearer,
                credential.reference(),
                credential,
                currentness,
                trusted_unix_seconds()?,
            )?;
            ensure_request_live(effective_deadline)?;
            return Err(error);
        }
        let response = invocation
            .decode_response(&reply.body)
            .map_err(|_| HarnessTransportError::InvalidOwnerResponse)?;
        ensure_request_live(effective_deadline)?;
        validate_live_authority(
            admission,
            invocation,
            AdmissionUse::ReadOnly,
            &bearer,
            credential.reference(),
            credential,
            currentness,
            trusted_unix_seconds()?,
        )?;
        ensure_request_live(effective_deadline)?;
        Ok(response)
    }
}

fn ensure_request_live(deadline: Instant) -> Result<(), HarnessTransportError> {
    if deadline <= Instant::now() {
        Err(HarnessTransportError::Deadline)
    } else {
        Ok(())
    }
}

fn persist_before_deadline<T>(
    deadline: Instant,
    persist: impl FnOnce() -> Result<T, HarnessTransportError>,
) -> Result<T, HarnessTransportError> {
    ensure_request_live(deadline)?;
    let value = persist()?;
    ensure_request_live(deadline)?;
    Ok(value)
}

fn trusted_unix_seconds() -> Result<u64, HarnessTransportError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HarnessTransportError::ClockUnavailable)?;
    let rounds_up = if elapsed.subsec_nanos() > 0 { 1 } else { 0 };
    Ok(elapsed.as_secs().saturating_add(rounds_up))
}

fn authorize_connection(
    admission: &TrustedInvocationAdmission,
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    bearer: &OneUseHarnessBearer,
    reference: &ProtectedAuthReference,
    credential: &ResolvedOwnerCredential,
    currentness: &mut dyn LiveInvocationCurrentness,
    request_deadline: Instant,
) -> Result<Instant, HarnessTransportError> {
    let checked_at = trusted_unix_seconds()?;
    validate_live_admission(
        admission, invocation, use_kind, bearer, reference, checked_at,
    )?;
    let now = trusted_unix_seconds()?;
    revalidate_currentness(
        currentness,
        admission,
        invocation,
        credential,
        use_kind,
        now,
    )?;
    admitted_deadline(admission, invocation, use_kind, now, request_deadline)
}

fn revalidate_currentness(
    currentness: &mut dyn LiveInvocationCurrentness,
    admission: &TrustedInvocationAdmission,
    invocation: &ContextOwnerInvocationV2,
    credential: &ResolvedOwnerCredential,
    use_kind: AdmissionUse,
    now: u64,
) -> Result<(), HarnessTransportError> {
    currentness
        .revalidate(admission, invocation, credential, use_kind, now)
        .map_err(|error| match error {
            LiveCurrentnessError::Unavailable => HarnessTransportError::CredentialUnavailable,
            LiveCurrentnessError::Denied => HarnessTransportError::CredentialDenied,
        })
}

fn validate_live_authority(
    admission: &TrustedInvocationAdmission,
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    bearer: &OneUseHarnessBearer,
    reference: &ProtectedAuthReference,
    credential: &ResolvedOwnerCredential,
    currentness: &mut dyn LiveInvocationCurrentness,
    now: u64,
) -> Result<(), HarnessTransportError> {
    validate_live_admission(admission, invocation, use_kind, bearer, reference, now)?;
    let currentness_at = trusted_unix_seconds()?;
    revalidate_currentness(
        currentness,
        admission,
        invocation,
        credential,
        use_kind,
        currentness_at,
    )
}

fn validate_live_admission(
    admission: &TrustedInvocationAdmission,
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    bearer: &OneUseHarnessBearer,
    reference: &ProtectedAuthReference,
    now: u64,
) -> Result<(), HarnessTransportError> {
    admission
        .validate_for(invocation, use_kind, now)
        .map_err(|_| HarnessTransportError::CredentialDenied)?;
    validate_bearer_binding(bearer, invocation, use_kind, reference, now)
}

fn admitted_deadline(
    admission: &TrustedInvocationAdmission,
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    now: u64,
    request_deadline: Instant,
) -> Result<Instant, HarnessTransportError> {
    let deadline = admission
        .deadline_for(invocation, use_kind, now)
        .map_err(|_| HarnessTransportError::CredentialDenied)?;
    Ok(deadline.min(request_deadline))
}
