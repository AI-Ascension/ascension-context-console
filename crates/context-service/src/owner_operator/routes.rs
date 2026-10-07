use crate::authenticated_ingress::AuthenticatedIngressError;
use crate::harness_context_owner_wire::{ContextOwnerInvocationV2, HarnessHttpMethod};
use crate::harness_facade::{HarnessFacadeConfig, RetentionPolicy, SecretDigest};
use crate::harness_owner_transport::{
    HarnessTransportError, InvocationSendContext, LiveCurrentnessError,
};
use crate::owner_invocation_store::{
    AdmissionUse, ReservationOutcome, required_console_permissions,
};
use std::net::TcpStream;
use std::time::Instant;
use zeroize::Zeroizing;

use super::currentness::Currentness;
use super::http::{ReadError, SensitiveRequest};
use super::service::OperatorService;

const INVOCATION_ROUTE: &str = "/v2/context-owner/invocations";
const LOOKUP_ROUTE: &str = "/v2/context-owner/invocations/receipt-lookup";
const CACHED_ROUTE: &str = "/v2/context-owner/invocations/cached-result";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RouteUse {
    Invoke,
    ExactLookup,
    CachedRead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RouteSelectionError {
    MethodNotAllowed,
    NotFound,
}

impl RouteSelectionError {
    pub(super) fn http_error(self) -> (u16, &'static [u8]) {
        match self {
            Self::MethodNotAllowed => (405, br#"{"error":"method_not_allowed"}"#),
            Self::NotFound => (404, br#"{"error":"route_unavailable"}"#),
        }
    }
}

fn response_error(status: u16, body: &'static [u8]) -> (u16, &'static [u8]) {
    (status, body)
}

pub(super) fn select_route(method: &str, target: &str) -> Result<RouteUse, RouteSelectionError> {
    if method != "POST" {
        return Err(RouteSelectionError::MethodNotAllowed);
    }
    match target {
        INVOCATION_ROUTE => Ok(RouteUse::Invoke),
        LOOKUP_ROUTE => Ok(RouteUse::ExactLookup),
        CACHED_ROUTE => Ok(RouteUse::CachedRead),
        _ => Err(RouteSelectionError::NotFound),
    }
}

pub(super) fn serve_request(
    service: &mut OperatorService,
    request: &SensitiveRequest,
    stream: &mut TcpStream,
    deadline: Instant,
) {
    if let Err((status, body)) = handle(service, request, stream, deadline) {
        let _ =
            super::http::write_response(stream, status, &Zeroizing::new(body.to_vec()), deadline);
    }
}

pub(super) fn read_error(error: ReadError) -> (u16, &'static [u8]) {
    match error {
        ReadError::BadRequest | ReadError::Deadline => (400, br#"{"error":"bad_request"}"#),
        ReadError::TooLarge => (413, br#"{"error":"request_too_large"}"#),
        ReadError::Io => (503, br#"{"error":"request_unavailable"}"#),
    }
}

fn handle(
    service: &mut OperatorService,
    request: &SensitiveRequest,
    stream: &mut TcpStream,
    deadline: Instant,
) -> Result<(), (u16, &'static [u8])> {
    if deadline <= Instant::now() {
        return Err((400, br#"{"error":"request_deadline_expired"}"#));
    }
    service
        ._process_lock
        .check_current()
        .map_err(|_| response_error(503, br#"{"error":"operator_lock_unavailable"}"#))?;
    let route = select_route(&request.0.method, &request.0.target)
        .map_err(RouteSelectionError::http_error)?;
    let invocation: ContextOwnerInvocationV2 =
        crate::harness_context_owner_wire::decode_bounded_json(&request.0.body)
            .map_err(|_| response_error(400, br#"{"error":"invalid_invocation"}"#))?;
    invocation
        .validate()
        .map_err(|_| response_error(400, br#"{"error":"invalid_invocation"}"#))?;
    validate_configured_identity(service, &invocation)?;
    let use_kind = select_use(&invocation, route)?;
    let facade = facade_for_request(&service.config)?;
    service
        .config
        .root
        .verify_current_root()
        .map_err(|_| response_error(503, br#"{"error":"operator_state_unavailable"}"#))?;
    let admitted = service
        .ingress
        .admit_owner_invocation(
            &request.0,
            &facade,
            &invocation,
            use_kind,
            &mut service.resolver,
        )
        .map_err(map_ingress_error)?;
    if deadline <= Instant::now() {
        return Err((409, br#"{"error":"request_deadline_expired"}"#));
    }
    service
        .config
        .root
        .verify_current_root()
        .map_err(|_| response_error(503, br#"{"error":"operator_state_unavailable"}"#))?;

    let OperatorService {
        config,
        _process_lock,
        ingress,
        resolver,
        redeemer,
        journal,
        journal_name,
        journal_identity,
        transport,
        ..
    } = service;
    let mut currentness = Currentness {
        ingress,
        resolver,
        config,
        request: &request.0,
        admitted: &admitted,
        journal_name,
        journal_identity: *journal_identity,
        process_lock: _process_lock,
    };
    let now = trusted_unix_seconds()
        .map_err(|_| response_error(503, br#"{"error":"clock_unavailable"}"#))?;
    let send_context = InvocationSendContext::borrowed(
        &invocation,
        admitted.admission(),
        admitted.credential(),
        deadline,
    );
    let response = match route {
        RouteUse::Invoke if use_kind == AdmissionUse::ReadOnly => transport
            .send_read_once(&send_context, redeemer, &mut currentness)
            .map_err(map_transport_error)?,
        RouteUse::Invoke => {
            config
                .root
                .verify_database_files(journal_name, Some(*journal_identity))
                .map_err(|_| response_error(503, br#"{"error":"operator_journal_unavailable"}"#))?;
            let reserved = journal
                .reserve(&invocation, admitted.admission(), now)
                .map_err(map_store_error)?;
            config
                .root
                .verify_database_files(journal_name, Some(*journal_identity))
                .map_err(|_| response_error(503, br#"{"error":"operator_journal_unavailable"}"#))?;
            let ReservationOutcome::Ready(reservation) = reserved else {
                return Err((409, br#"{"error":"exact_recovery_required"}"#));
            };
            transport
                .send_reserved_write(
                    journal,
                    reservation,
                    &send_context,
                    redeemer,
                    &mut currentness,
                )
                .map_err(map_transport_error)?
        }
        RouteUse::ExactLookup => {
            config
                .root
                .verify_database_files(journal_name, Some(*journal_identity))
                .map_err(|_| response_error(503, br#"{"error":"operator_journal_unavailable"}"#))?;
            let reserved = journal
                .reserve(&invocation, admitted.admission(), now)
                .map_err(map_store_error)?;
            config
                .root
                .verify_database_files(journal_name, Some(*journal_identity))
                .map_err(|_| response_error(503, br#"{"error":"operator_journal_unavailable"}"#))?;
            let ReservationOutcome::LookupRequired(reservation) = reserved else {
                return Err((409, br#"{"error":"exact_recovery_not_ready"}"#));
            };
            match transport
                .send_reserved_lookup(
                    journal,
                    reservation,
                    &send_context,
                    redeemer,
                    &mut currentness,
                )
                .map_err(map_transport_error)?
            {
                Some(response) => response,
                None => return Err((409, br#"{"error":"receipt_not_recorded"}"#)),
            }
        }
        RouteUse::CachedRead => {
            config
                .root
                .verify_database_files(journal_name, Some(*journal_identity))
                .map_err(|_| response_error(503, br#"{"error":"operator_journal_unavailable"}"#))?;
            let reserved = journal
                .reserve(&invocation, admitted.admission(), now)
                .map_err(map_store_error)?;
            config
                .root
                .verify_database_files(journal_name, Some(*journal_identity))
                .map_err(|_| response_error(503, br#"{"error":"operator_journal_unavailable"}"#))?;
            let ReservationOutcome::Cached(response) = reserved else {
                return Err((409, br#"{"error":"cached_result_unavailable"}"#));
            };
            currentness
                .check(
                    admitted.admission(),
                    &invocation,
                    admitted.credential(),
                    AdmissionUse::CachedRead,
                    trusted_unix_seconds()
                        .map_err(|_| response_error(503, br#"{"error":"clock_unavailable"}"#))?,
                )
                .map_err(map_currentness_error)?;
            if deadline <= Instant::now() {
                return Err((409, br#"{"error":"request_deadline_expired"}"#));
            }
            *response
        }
    };
    let body = super::http::serialize_bounded(&response)
        .map_err(|_| response_error(413, br#"{"error":"response_too_large"}"#))?;
    currentness
        .check(
            admitted.admission(),
            &invocation,
            admitted.credential(),
            use_kind,
            trusted_unix_seconds()
                .map_err(|_| response_error(503, br#"{"error":"clock_unavailable"}"#))?,
        )
        .map_err(map_currentness_error)?;
    if deadline <= Instant::now() {
        return Err((409, br#"{"error":"request_deadline_expired"}"#));
    }
    config
        .root
        .verify_current_root()
        .map_err(|_| response_error(503, br#"{"error":"operator_state_unavailable"}"#))?;
    let _ = super::http::write_response(stream, 200, &body, deadline);
    Ok(())
}

fn select_use(
    invocation: &ContextOwnerInvocationV2,
    route: RouteUse,
) -> Result<AdmissionUse, (u16, &'static [u8])> {
    match route {
        RouteUse::ExactLookup => Ok(AdmissionUse::ExactLookup),
        RouteUse::CachedRead => Ok(AdmissionUse::CachedRead),
        RouteUse::Invoke => {
            let endpoint = invocation
                .endpoint()
                .map_err(|_| response_error(400, br#"{"error":"invalid_invocation"}"#))?;
            let candidate = if endpoint.method == HarnessHttpMethod::Get {
                AdmissionUse::ReadOnly
            } else {
                AdmissionUse::Write
            };
            required_console_permissions(&invocation.operation, candidate)
                .map(|_| candidate)
                .map_err(|_| response_error(403, br#"{"error":"operation_not_enabled"}"#))
        }
    }
}

fn validate_configured_identity(
    service: &OperatorService,
    invocation: &ContextOwnerInvocationV2,
) -> Result<(), (u16, &'static [u8])> {
    let scope = &invocation.identity.console_scope;
    if invocation.identity.harness.owner_id != service.config.owner_id
        || scope.project_id != service.config.scope.project_id
        || scope.run_id != service.config.scope.run_id
        || scope.episode_id != service.config.scope.episode_id
        || scope.agent_id != service.config.scope.agent_id
        || invocation
            .expected_binding
            .as_ref()
            .is_some_and(|binding| binding.owner_id != service.config.owner_id)
    {
        return Err((403, br#"{"error":"owner_scope_mismatch"}"#));
    }
    Ok(())
}

fn facade_for_request(
    config: &super::config::OperatorConfig,
) -> Result<HarnessFacadeConfig, (u16, &'static [u8])> {
    let csrf = config
        .root
        .read_file(&config.csrf_secret_ref, 4 * 1024)
        .map_err(|_| response_error(503, br#"{"error":"operator_csrf_unavailable"}"#))?;
    let digest = SecretDigest::from_secret(&csrf)
        .map_err(|_| response_error(503, br#"{"error":"operator_csrf_unavailable"}"#))?;
    HarnessFacadeConfig::new(
        config.scope.clone(),
        config.expected_host.clone(),
        config.expected_origin.clone(),
        Some(digest),
        RetentionPolicy::default(),
    )
    .map_err(|_| response_error(503, br#"{"error":"operator_configuration_invalid"}"#))
}

fn map_ingress_error(error: AuthenticatedIngressError) -> (u16, &'static [u8]) {
    match error {
        AuthenticatedIngressError::InvalidHttpRequest => (400, br#"{"error":"invalid_request"}"#),
        AuthenticatedIngressError::InvalidBearer | AuthenticatedIngressError::Unauthorized => {
            (401, br#"{"error":"unauthorized"}"#)
        }
        AuthenticatedIngressError::GrantDenied
        | AuthenticatedIngressError::OwnerCredentialDenied => (403, br#"{"error":"forbidden"}"#),
        AuthenticatedIngressError::InvalidConfiguration
        | AuthenticatedIngressError::InvalidOwnerCredential => {
            (400, br#"{"error":"invalid_request"}"#)
        }
        _ => (503, br#"{"error":"authorization_unavailable"}"#),
    }
}

fn map_transport_error(error: HarnessTransportError) -> (u16, &'static [u8]) {
    match error {
        HarnessTransportError::CredentialDenied => (403, br#"{"error":"forbidden"}"#),
        HarnessTransportError::InvalidInvocation | HarnessTransportError::UnsupportedOperation => {
            (400, br#"{"error":"invalid_operation"}"#)
        }
        HarnessTransportError::Deadline
        | HarnessTransportError::IoOutcomeUnknown
        | HarnessTransportError::InvalidFraming
        | HarnessTransportError::ResponseTooLarge
        | HarnessTransportError::ControlReceiptNotRecorded
        | HarnessTransportError::RemoteManagement { .. }
        | HarnessTransportError::Store(crate::owner_invocation_store::StoreError::Conflict)
        | HarnessTransportError::Store(
            crate::owner_invocation_store::StoreError::RecoveryRequired,
        )
        | HarnessTransportError::Store(
            crate::owner_invocation_store::StoreError::AmbiguousWithoutExactLookup,
        ) => (409, br#"{"error":"exact_recovery_required"}"#),
        _ => (503, br#"{"error":"owner_unavailable"}"#),
    }
}

fn map_store_error(error: crate::owner_invocation_store::StoreError) -> (u16, &'static [u8]) {
    match error {
        crate::owner_invocation_store::StoreError::Invalid
        | crate::owner_invocation_store::StoreError::Denied
        | crate::owner_invocation_store::StoreError::UnsupportedOperation => {
            (403, br#"{"error":"operation_not_authorized"}"#)
        }
        crate::owner_invocation_store::StoreError::Conflict
        | crate::owner_invocation_store::StoreError::RecoveryRequired
        | crate::owner_invocation_store::StoreError::AmbiguousWithoutExactLookup
        | crate::owner_invocation_store::StoreError::AttemptLimit => {
            (409, br#"{"error":"exact_recovery_required"}"#)
        }
        _ => (503, br#"{"error":"owner_journal_unavailable"}"#),
    }
}

fn map_currentness_error(error: LiveCurrentnessError) -> (u16, &'static [u8]) {
    match error {
        LiveCurrentnessError::Denied => (403, br#"{"error":"authorization_changed"}"#),
        LiveCurrentnessError::Unavailable => (503, br#"{"error":"authorization_unavailable"}"#),
    }
}

fn trusted_unix_seconds() -> Result<u64, ()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ())?;
    Ok(now
        .as_secs()
        .saturating_add(u64::from(now.subsec_nanos() != 0)))
}
