use super::*;
use crate::owner_invocation_store::{
    ExactReservation, OneUseSendPermit, OwnerInvocationKeyProvider, OwnerInvocationStore,
};

use super::authorization::{mark_write_unknown, validate_send_permit};
use super::errors::management_error;

impl HarnessOwnerTransport {
    /// Consume the only write permit after its store claim has committed. Any failed or
    /// ambiguous exchange marks the record unknown; this method never returns a retry permit.
    pub(crate) fn send_reserved_write<
        K: OwnerInvocationKeyProvider,
        R: HarnessCredentialRedeemer,
    >(
        &self,
        store: &mut OwnerInvocationStore<K>,
        reservation: ExactReservation,
        request: &InvocationSendContext<'_>,
        redeemer: &mut R,
        currentness: &mut dyn LiveInvocationCurrentness,
    ) -> Result<HarnessResponseV1, HarnessTransportError> {
        let request = request.with_deadline(framing::operation_deadline(
            self.config.deadline,
            request.request_deadline,
        )?);
        ensure_request_live(request.request_deadline)?;
        let now = trusted_unix_seconds()?;
        request
            .admission
            .validate_for(request.invocation, AdmissionUse::Write, now)
            .map_err(|_| HarnessTransportError::CredentialDenied)?;
        ensure_request_live(request.request_deadline)?;
        let bearer = self.redeem_for_invocation(
            request.invocation,
            request.credential,
            redeemer,
            AdmissionUse::Write,
            trusted_unix_seconds()?,
        )?;
        let now = trusted_unix_seconds()?;
        validate_live_authority(
            &request,
            AdmissionUse::Write,
            &bearer,
            request.credential.reference(),
            currentness,
            now,
        )?;
        ensure_request_live(request.request_deadline)?;
        let request = request.with_deadline(admitted_deadline(&request, AdmissionUse::Write, now)?);
        ensure_request_live(request.request_deadline)?;
        let permit = store
            .claim_send(reservation, request.admission, now)
            .map_err(HarnessTransportError::Store)?;
        self.send_claimed_write(store, permit, bearer, request.after_claim(now), currentness)
    }

    fn send_claimed_write<K: OwnerInvocationKeyProvider>(
        &self,
        store: &mut OwnerInvocationStore<K>,
        permit: OneUseSendPermit,
        bearer: OneUseHarnessBearer,
        claimed: ClaimedInvocationSendContext<'_>,
        currentness: &mut dyn LiveInvocationCurrentness,
    ) -> Result<HarnessResponseV1, HarnessTransportError> {
        let invocation = permit.invocation().clone();
        let endpoint = permit.endpoint().clone();
        let reference = permit.protected_reference().clone();
        let request = claimed.request.for_invocation(&invocation);
        let claimed_at = claimed.claimed_at;
        let validate_sealed =
            |now| validate_live_admission(&request, AdmissionUse::Write, &bearer, &reference, now);
        let now = match trusted_unix_seconds() {
            Ok(now) => now,
            Err(error) => return Err(mark_write_unknown(store, permit, claimed_at, error)),
        };
        let preflight = request
            .admission
            .validate_for(&invocation, AdmissionUse::Write, now)
            .map_err(|_| HarnessTransportError::CredentialDenied)
            .and_then(|()| validate_send_permit(&invocation, &endpoint, permit.body()))
            .and_then(|()| {
                validate_bearer_binding(&bearer, &invocation, AdmissionUse::Write, &reference, now)
            });
        if let Err(error) = preflight {
            return Err(mark_write_unknown(store, permit, now, error));
        }
        let request = match admitted_deadline(&request, AdmissionUse::Write, now) {
            Ok(deadline) => request.with_deadline(deadline),
            Err(error) => return Err(mark_write_unknown(store, permit, now, error)),
        };
        if let Err(error) = ensure_request_live(request.request_deadline) {
            return Err(mark_write_unknown(store, permit, now, error));
        }
        let reply = match framing::exchange_with_authorization(
            &self.config,
            &endpoint,
            Some(permit.body()),
            &bearer.token,
            request.request_deadline,
            || {
                authorize_connection(
                    &request,
                    AdmissionUse::Write,
                    &bearer,
                    &reference,
                    currentness,
                )
            },
        ) {
            Ok(reply) => reply,
            Err(error) => {
                let failed_at = trusted_unix_seconds().unwrap_or(claimed_at);
                return Err(mark_write_unknown(store, permit, failed_at, error));
            }
        };
        let response_at = match trusted_unix_seconds() {
            Ok(now) => now,
            Err(error) => return Err(mark_write_unknown(store, permit, claimed_at, error)),
        };
        if let Err(error) = ensure_request_live(request.request_deadline) {
            return Err(mark_write_unknown(store, permit, response_at, error));
        }
        if let Err(error) = validate_sealed(response_at) {
            return Err(mark_write_unknown(store, permit, response_at, error));
        }
        if reply.status != 200 {
            let error = management_error(reply.status, &reply.body, &invocation.operation);
            if let Err(store_error) = store.mark_send_unknown(permit, response_at) {
                return Err(HarnessTransportError::Store(store_error));
            }
            validate_live_authority(
                &request,
                AdmissionUse::Write,
                &bearer,
                &reference,
                currentness,
                trusted_unix_seconds()?,
            )?;
            ensure_request_live(request.request_deadline)?;
            return Err(error);
        }
        let completed_at = match trusted_unix_seconds() {
            Ok(now) => now,
            Err(error) => return Err(mark_write_unknown(store, permit, response_at, error)),
        };
        if let Err(error) = validate_sealed(completed_at) {
            return Err(mark_write_unknown(store, permit, completed_at, error));
        }
        if let Err(error) = ensure_request_live(request.request_deadline) {
            return Err(mark_write_unknown(store, permit, completed_at, error));
        }
        let response = persist_before_deadline(request.request_deadline, || {
            store
                .complete_send(permit, &reply.body, completed_at)
                .map_err(HarnessTransportError::Store)
        })?;
        validate_live_authority(
            &request,
            AdmissionUse::Write,
            &bearer,
            &reference,
            currentness,
            trusted_unix_seconds()?,
        )?;
        ensure_request_live(request.request_deadline)?;
        Ok(response)
    }

    /// Consume one exact-receipt lookup permit. A control lookup's pinned 404 is an explicit
    /// no-receipt result and remains unknown; it is never represented as JSON null or success.
    pub(crate) fn send_reserved_lookup<
        K: OwnerInvocationKeyProvider,
        R: HarnessCredentialRedeemer,
    >(
        &self,
        store: &mut OwnerInvocationStore<K>,
        reservation: ExactReservation,
        request: &InvocationSendContext<'_>,
        redeemer: &mut R,
        currentness: &mut dyn LiveInvocationCurrentness,
    ) -> Result<Option<HarnessResponseV1>, HarnessTransportError> {
        let request = request.with_deadline(framing::operation_deadline(
            self.config.deadline,
            request.request_deadline,
        )?);
        ensure_request_live(request.request_deadline)?;
        let now = trusted_unix_seconds()?;
        request
            .admission
            .validate_for(request.invocation, AdmissionUse::ExactLookup, now)
            .map_err(|_| HarnessTransportError::CredentialDenied)?;
        ensure_request_live(request.request_deadline)?;
        let bearer = self.redeem_for_invocation(
            request.invocation,
            request.credential,
            redeemer,
            AdmissionUse::ExactLookup,
            trusted_unix_seconds()?,
        )?;
        let now = trusted_unix_seconds()?;
        validate_live_authority(
            &request,
            AdmissionUse::ExactLookup,
            &bearer,
            request.credential.reference(),
            currentness,
            now,
        )?;
        ensure_request_live(request.request_deadline)?;
        let request =
            request.with_deadline(admitted_deadline(&request, AdmissionUse::ExactLookup, now)?);
        ensure_request_live(request.request_deadline)?;
        let permit = store
            .claim_exact_lookup(reservation, request.invocation, request.admission, now)
            .map_err(HarnessTransportError::Store)?;
        self.send_claimed_lookup(store, permit, bearer, request.after_claim(now), currentness)
    }
}
