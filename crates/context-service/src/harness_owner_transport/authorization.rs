use super::{
    AdmissionUse, ContextOwnerEndpointV1, ContextOwnerInvocationV2, HarnessHttpMethod,
    HarnessOwnerTransport, HarnessResponseV1, HarnessTransportError, LiveInvocationCurrentness,
    MAX_BEARER_BYTES, MAX_HARNESS_JSON_BODY_BYTES, MAX_PATH_BYTES, MAX_QUERY_ITEMS,
    OneUseHarnessBearer, ProtectedAuthReference,
};
use crate::harness_context_owner_wire::ContextOwnerOperationV2;
use crate::owner_invocation_store::{
    OneUseLookupPermit, OneUseSendPermit, OwnerInvocationKeyProvider, OwnerInvocationStore,
    TrustedInvocationAdmission, lookup_invocation,
};
use crate::protected_owner_credentials::ResolvedOwnerCredential;
use std::time::Instant;
use zeroize::Zeroizing;

/// Borrows the exact invocation authority and carries the caller's absolute request deadline.
/// Bundling these existing values creates no admission or credential authority.
#[derive(Clone, Copy)]
pub(crate) struct InvocationSendContext<'a> {
    pub(super) invocation: &'a ContextOwnerInvocationV2,
    pub(super) admission: &'a TrustedInvocationAdmission,
    pub(super) credential: &'a ResolvedOwnerCredential,
    pub(super) request_deadline: Instant,
}

pub(super) struct ClaimedInvocationSendContext<'a> {
    pub(super) request: InvocationSendContext<'a>,
    pub(super) claimed_at: u64,
}

impl<'a> InvocationSendContext<'a> {
    pub(crate) fn borrowed(
        invocation: &'a ContextOwnerInvocationV2,
        admission: &'a TrustedInvocationAdmission,
        credential: &'a ResolvedOwnerCredential,
        request_deadline: Instant,
    ) -> Self {
        Self {
            invocation,
            admission,
            credential,
            request_deadline,
        }
    }

    pub(super) fn with_deadline(&self, deadline: Instant) -> Self {
        Self {
            request_deadline: self.request_deadline.min(deadline),
            ..*self
        }
    }

    pub(super) fn after_claim(&self, claimed_at: u64) -> ClaimedInvocationSendContext<'a> {
        ClaimedInvocationSendContext {
            request: *self,
            claimed_at,
        }
    }

    pub(super) fn for_invocation<'b>(
        &'b self,
        invocation: &'b ContextOwnerInvocationV2,
    ) -> InvocationSendContext<'b> {
        InvocationSendContext {
            invocation,
            admission: self.admission,
            credential: self.credential,
            request_deadline: self.request_deadline,
        }
    }
}

pub(super) fn validate_bearer(token: &[u8]) -> Result<(), HarnessTransportError> {
    if token.is_empty()
        || token.len() > MAX_BEARER_BYTES
        || token.iter().any(|byte| !(0x21..=0x7e).contains(byte))
    {
        return Err(HarnessTransportError::CredentialDenied);
    }
    Ok(())
}

pub(super) fn validate_bearer_binding(
    bearer: &OneUseHarnessBearer,
    invocation: &ContextOwnerInvocationV2,
    use_kind: AdmissionUse,
    reference: &ProtectedAuthReference,
    now: u64,
) -> Result<(), HarnessTransportError> {
    if bearer.invocation != *invocation
        || bearer.use_kind != use_kind
        || bearer.reference != *reference
        || bearer.expires_at <= now
        || bearer.required_scopes
            != super::required_harness_scopes(&invocation.operation, use_kind)
                .map_err(|_| HarnessTransportError::CredentialDenied)?
    {
        return Err(HarnessTransportError::CredentialDenied);
    }
    validate_bearer(&bearer.token)
}

pub(super) fn validate_closed_endpoint(
    invocation: &ContextOwnerInvocationV2,
    endpoint: &ContextOwnerEndpointV1,
) -> Result<(), HarnessTransportError> {
    let derived = invocation
        .operation
        .endpoint()
        .map_err(|_| HarnessTransportError::InvalidInvocation)?;
    if &derived != endpoint
        || endpoint.path.len() > MAX_PATH_BYTES
        || !endpoint.path.starts_with("/v1/workflow-runs/")
        || endpoint.path.contains(['?', '#', '\\', '\r', '\n'])
        || endpoint.query.len() > MAX_QUERY_ITEMS
    {
        return Err(HarnessTransportError::InvalidInvocation);
    }
    let mut names = std::collections::BTreeSet::new();
    for query in &endpoint.query {
        if !matches!(
            query.name.as_str(),
            "draft_id" | "include_content" | "after_revision_id" | "limit"
        ) || !safe_component(&query.name)
            || !safe_component(&query.value)
            || !names.insert(query.name.as_str())
        {
            return Err(HarnessTransportError::InvalidInvocation);
        }
    }
    Ok(())
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

pub(super) fn validate_send_permit(
    invocation: &ContextOwnerInvocationV2,
    endpoint: &ContextOwnerEndpointV1,
    body: &[u8],
) -> Result<(), HarnessTransportError> {
    validate_closed_endpoint(invocation, endpoint)?;
    let expected_body = Zeroizing::new(
        invocation
            .harness_body()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?
            .ok_or(HarnessTransportError::InvalidInvocation)?,
    );
    if !is_supported_write(&invocation.operation)
        || endpoint.method == HarnessHttpMethod::Get
        || expected_body.as_slice() != body
        || body.len() > MAX_HARNESS_JSON_BODY_BYTES
    {
        return Err(HarnessTransportError::InvalidInvocation);
    }
    Ok(())
}

pub(super) fn validate_lookup_permit(
    invocation: &ContextOwnerInvocationV2,
    endpoint: &ContextOwnerEndpointV1,
    body: &[u8],
) -> Result<(), HarnessTransportError> {
    validate_closed_endpoint(invocation, endpoint)?;
    let expected_body = Zeroizing::new(
        invocation
            .harness_body()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?
            .ok_or(HarnessTransportError::InvalidInvocation)?,
    );
    if !matches!(
        &invocation.operation,
        ContextOwnerOperationV2::LookupMutation { .. }
            | ContextOwnerOperationV2::LookupPublication { .. }
            | ContextOwnerOperationV2::LookupControl { .. }
    ) || endpoint.method != HarnessHttpMethod::Post
        || expected_body.as_slice() != body
        || body.len() > MAX_HARNESS_JSON_BODY_BYTES
    {
        return Err(HarnessTransportError::InvalidInvocation);
    }
    Ok(())
}

pub(super) fn validate_derived_lookup_permit(
    original: &ContextOwnerInvocationV2,
    lookup: &ContextOwnerInvocationV2,
    endpoint: &ContextOwnerEndpointV1,
    body: &[u8],
) -> Result<(), HarnessTransportError> {
    let expected = lookup_invocation(original, original)
        .map_err(|_| HarnessTransportError::InvalidInvocation)?;
    if &expected != lookup {
        return Err(HarnessTransportError::InvalidInvocation);
    }
    let expected_endpoint = expected
        .endpoint()
        .map_err(|_| HarnessTransportError::InvalidInvocation)?;
    let expected_body = Zeroizing::new(
        expected
            .harness_body()
            .map_err(|_| HarnessTransportError::InvalidInvocation)?
            .ok_or(HarnessTransportError::InvalidInvocation)?,
    );
    if &expected_endpoint != endpoint || expected_body.as_slice() != body {
        return Err(HarnessTransportError::InvalidInvocation);
    }
    validate_lookup_permit(lookup, endpoint, body)
}

fn is_supported_write(operation: &ContextOwnerOperationV2) -> bool {
    matches!(
        operation,
        ContextOwnerOperationV2::CreateDraft { .. }
            | ContextOwnerOperationV2::PatchDraft { .. }
            | ContextOwnerOperationV2::CreatePreview { .. }
            | ContextOwnerOperationV2::PublishDraft { .. }
            | ContextOwnerOperationV2::UploadSource { .. }
            | ContextOwnerOperationV2::SubmitControl { .. }
    )
}

pub(super) fn mark_write_unknown<K: OwnerInvocationKeyProvider>(
    store: &mut OwnerInvocationStore<K>,
    permit: OneUseSendPermit,
    now: u64,
    error: HarnessTransportError,
) -> HarnessTransportError {
    match store.mark_send_unknown(permit, now) {
        Ok(()) => error,
        Err(store_error) => HarnessTransportError::Store(store_error),
    }
}

pub(super) fn mark_lookup_unknown<K: OwnerInvocationKeyProvider>(
    store: &mut OwnerInvocationStore<K>,
    permit: OneUseLookupPermit,
    now: u64,
    error: HarnessTransportError,
) -> HarnessTransportError {
    match store.mark_lookup_unknown(permit, now) {
        Ok(()) => error,
        Err(store_error) => HarnessTransportError::Store(store_error),
    }
}

impl HarnessOwnerTransport {
    /// Finish one claimed exact lookup. Any framing or validation failure consumes the permit and
    /// leaves the original write recoverable only through another exact lookup.
    pub(super) fn send_claimed_lookup<K: OwnerInvocationKeyProvider>(
        &self,
        store: &mut OwnerInvocationStore<K>,
        permit: OneUseLookupPermit,
        bearer: OneUseHarnessBearer,
        claimed: ClaimedInvocationSendContext<'_>,
        currentness: &mut dyn LiveInvocationCurrentness,
    ) -> Result<Option<HarnessResponseV1>, HarnessTransportError> {
        let admitted_invocation = permit.original_invocation().clone();
        let lookup_invocation = permit.invocation().clone();
        let endpoint = permit.endpoint().clone();
        let reference = permit.protected_reference().clone();
        let request = claimed.request.for_invocation(&admitted_invocation);
        let claimed_at = claimed.claimed_at;
        let validate_sealed = |now| {
            super::validate_live_admission(
                &request,
                AdmissionUse::ExactLookup,
                &bearer,
                &reference,
                now,
            )
        };
        let now = match super::trusted_unix_seconds() {
            Ok(now) => now,
            Err(error) => return Err(mark_lookup_unknown(store, permit, claimed_at, error)),
        };
        let preflight = request
            .admission
            .validate_for(&admitted_invocation, AdmissionUse::ExactLookup, now)
            .map_err(|_| HarnessTransportError::CredentialDenied)
            .and_then(|()| {
                validate_derived_lookup_permit(
                    &admitted_invocation,
                    &lookup_invocation,
                    &endpoint,
                    permit.body(),
                )
            })
            .and_then(|()| {
                validate_bearer_binding(
                    &bearer,
                    &admitted_invocation,
                    AdmissionUse::ExactLookup,
                    &reference,
                    now,
                )
            });
        if let Err(error) = preflight {
            return Err(mark_lookup_unknown(store, permit, now, error));
        }
        let request = match super::admitted_deadline(&request, AdmissionUse::ExactLookup, now) {
            Ok(deadline) => request.with_deadline(deadline),
            Err(error) => return Err(mark_lookup_unknown(store, permit, now, error)),
        };
        if let Err(error) = super::ensure_request_live(request.request_deadline) {
            return Err(mark_lookup_unknown(store, permit, now, error));
        }
        let reply = match super::framing::exchange_with_authorization(
            &self.config,
            &endpoint,
            Some(permit.body()),
            &bearer.token,
            request.request_deadline,
            || {
                super::authorize_connection(
                    &request,
                    AdmissionUse::ExactLookup,
                    &bearer,
                    &reference,
                    currentness,
                )
            },
        ) {
            Ok(reply) => reply,
            Err(error) => {
                let failed_at = super::trusted_unix_seconds().unwrap_or(claimed_at);
                return Err(mark_lookup_unknown(store, permit, failed_at, error));
            }
        };
        let response_at = match super::trusted_unix_seconds() {
            Ok(now) => now,
            Err(error) => return Err(mark_lookup_unknown(store, permit, claimed_at, error)),
        };
        if let Err(error) = super::ensure_request_live(request.request_deadline) {
            return Err(mark_lookup_unknown(store, permit, response_at, error));
        }
        if let Err(error) = validate_sealed(response_at) {
            return Err(mark_lookup_unknown(store, permit, response_at, error));
        }
        if reply.status != 200 {
            let error = super::errors::management_error(
                reply.status,
                &reply.body,
                &lookup_invocation.operation,
            );
            if let Err(store_error) = store.mark_lookup_unknown(permit, response_at) {
                return Err(HarnessTransportError::Store(store_error));
            }
            super::validate_live_authority(
                &request,
                AdmissionUse::ExactLookup,
                &bearer,
                &reference,
                currentness,
                super::trusted_unix_seconds()?,
            )?;
            super::ensure_request_live(request.request_deadline)?;
            return Err(error);
        }
        let completed_at = match super::trusted_unix_seconds() {
            Ok(now) => now,
            Err(error) => return Err(mark_lookup_unknown(store, permit, response_at, error)),
        };
        if let Err(error) = validate_sealed(completed_at) {
            return Err(mark_lookup_unknown(store, permit, completed_at, error));
        }
        if let Err(error) = super::ensure_request_live(request.request_deadline) {
            return Err(mark_lookup_unknown(store, permit, completed_at, error));
        }
        let response = super::persist_before_deadline(request.request_deadline, || {
            store
                .complete_lookup(permit, &reply.body, completed_at)
                .map_err(HarnessTransportError::Store)
        })?;
        super::validate_live_authority(
            &request,
            AdmissionUse::ExactLookup,
            &bearer,
            &reference,
            currentness,
            super::trusted_unix_seconds()?,
        )?;
        super::ensure_request_live(request.request_deadline)?;
        Ok(response)
    }
}
