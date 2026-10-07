use super::*;
use crate::control::Scope;
use crate::harness_context_owner_wire::{
    CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2, ConsoleOwnerIdentityV2, ConsoleOwnerScopeV2,
    ContextControlCommand, ContextOwnerItemsQueryV1, ContextOwnerOperationV2,
    HarnessActorIdentityV2, OwnerIdentityCorrelationV2,
};
use crate::harness_facade::ProtectedAuthReference;
use crate::protected_owner_credentials::OwnerCredentialDescriptor;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub(super) fn source_status_invocation() -> ContextOwnerInvocationV2 {
    let scope = ConsoleOwnerScopeV2 {
        project_id: "project-1".to_owned(),
        run_id: "run-1".to_owned(),
        episode_id: "episode-1".to_owned(),
        agent_id: "agent-1".to_owned(),
    };
    ContextOwnerInvocationV2 {
        schema_version: CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2.to_owned(),
        identity: OwnerIdentityCorrelationV2 {
            console: ConsoleOwnerIdentityV2 {
                issuer: "issuer.example".to_owned(),
                subject: "subject-1".to_owned(),
                audience: "console-api".to_owned(),
                credential_id: "console-cred-1".to_owned(),
                grant_id: "grant-1".to_owned(),
                grant_generation: 1,
                grant_expires_at: 2_000,
            },
            console_scope: scope,
            harness: HarnessActorIdentityV2 {
                actor_subject: "subject-1".to_owned(),
                owner_id: "owner-1".to_owned(),
                workflow_run_id: "run-1".to_owned(),
                credential_reference_id: "ref-1".to_owned(),
                credential_expires_at: 1_900,
            },
        },
        expected_binding: None,
        operation: ContextOwnerOperationV2::CurrentSourceStatus {
            workflow_run_id: "run-1".to_owned(),
        },
    }
}

pub(super) fn credential(
    invocation: &ContextOwnerInvocationV2,
    scopes: &[&str],
    expires_at: u64,
) -> ResolvedOwnerCredential {
    let identity = &invocation.identity;
    let descriptor = OwnerCredentialDescriptor::new(
        identity.console.issuer.clone(),
        identity.console.subject.clone(),
        identity.harness.actor_subject.clone(),
        Scope {
            project_id: identity.console_scope.project_id.clone(),
            run_id: identity.console_scope.run_id.clone(),
            episode_id: identity.console_scope.episode_id.clone(),
            agent_id: identity.console_scope.agent_id.clone(),
        },
        scopes
            .iter()
            .map(|scope| (*scope).to_owned())
            .collect::<Vec<_>>(),
        expires_at,
    );
    ResolvedOwnerCredential::new(
        ProtectedAuthReference::new(identity.harness.credential_reference_id.clone()).unwrap(),
        descriptor,
    )
}

#[derive(Default)]
pub(super) struct TestRedeemer {
    calls: usize,
    failure: Option<CredentialRedemptionError>,
    seen_reference: Option<String>,
    seen_scopes: Vec<String>,
}

impl HarnessCredentialRedeemer for TestRedeemer {
    fn redeem(
        &mut self,
        reference: &ProtectedAuthReference,
        _descriptor: &OwnerCredentialDescriptor,
        _invocation: &ContextOwnerInvocationV2,
        required_scopes: &[&'static str],
        _now: u64,
    ) -> Result<Zeroizing<Vec<u8>>, CredentialRedemptionError> {
        self.calls += 1;
        self.seen_reference = Some(reference.as_str().to_owned());
        self.seen_scopes = required_scopes
            .iter()
            .map(|scope| (*scope).to_owned())
            .collect();
        if let Some(error) = self.failure {
            return Err(error);
        }
        Ok(Zeroizing::new(b"ephemeral-local-test-token".to_vec()))
    }
}

pub(super) fn transport_for(address: SocketAddr, deadline: Duration) -> HarnessOwnerTransport {
    let config = HarnessOwnerTransportConfig::new(address, deadline).unwrap();
    HarnessOwnerTransport::new(config)
}

#[test]
fn transport_configuration_accepts_only_bounded_numeric_loopback() {
    assert!(
        HarnessOwnerTransportConfig::new(
            SocketAddr::from(([127, 0, 0, 1], 43129)),
            Duration::from_secs(1),
        )
        .is_ok()
    );
    assert_eq!(
        HarnessOwnerTransportConfig::new(
            SocketAddr::from(([192, 0, 2, 1], 43129)),
            Duration::from_secs(1),
        )
        .err()
        .unwrap(),
        HarnessTransportError::InvalidConfiguration
    );
    assert!(
        HarnessOwnerTransportConfig::new(
            SocketAddr::from(([127, 0, 0, 1], 0)),
            Duration::from_secs(1),
        )
        .is_err()
    );
    assert!(
        HarnessOwnerTransportConfig::new(
            SocketAddr::from(([127, 0, 0, 1], 43129)),
            Duration::from_secs(6),
        )
        .is_err()
    );
}

#[test]
fn operation_budget_is_captured_before_preflight_and_not_restarted() {
    let transport = transport_for(
        SocketAddr::from(([127, 0, 0, 1], 43129)),
        Duration::from_millis(100),
    );
    let caller_deadline = Instant::now() + Duration::from_secs(2);
    let operation_deadline =
        framing::operation_deadline(transport.config.deadline, caller_deadline).unwrap();
    assert!(operation_deadline < caller_deadline);

    // A slow authentication/redeemer/store preflight consumes the original budget. Starting a
    // fresh config timeout after that work would incorrectly extend the operation.
    thread::sleep(Duration::from_millis(120));
    assert_eq!(
        ensure_request_live(operation_deadline),
        Err(HarnessTransportError::Deadline)
    );
    assert!(Instant::now() + transport.config.deadline > operation_deadline);
}

#[test]
fn delayed_store_completion_keeps_persisted_result_but_denies_late_success() {
    let deadline = Instant::now() + Duration::from_millis(60);
    let persisted = std::cell::Cell::new(false);
    let result = persist_before_deadline(deadline, || {
        // Model SQLite commit latency after a Harness response has already arrived.
        thread::sleep(Duration::from_millis(90));
        persisted.set(true);
        Ok(())
    });

    assert_eq!(result, Err(HarnessTransportError::Deadline));
    assert!(
        persisted.get(),
        "late completion remains durably available for recovery"
    );
}

#[test]
fn credential_identity_scope_reference_and_expiry_fail_before_redemption() {
    let invocation = source_status_invocation();
    let address = SocketAddr::from(([127, 0, 0, 1], 43129));
    let transport = transport_for(address, Duration::from_secs(1));
    let mut redeemer = TestRedeemer::default();

    let wrong_subject = ResolvedOwnerCredential::new(
        ProtectedAuthReference::new("ref-1").unwrap(),
        OwnerCredentialDescriptor::new(
            "issuer.example",
            "other-subject",
            "subject-1",
            Scope {
                project_id: "project-1".to_owned(),
                run_id: "run-1".to_owned(),
                episode_id: "episode-1".to_owned(),
                agent_id: "agent-1".to_owned(),
            },
            vec!["workflow:read".to_owned()],
            1_900,
        ),
    );
    assert_eq!(
        transport
            .redeem_for_invocation(
                &invocation,
                &wrong_subject,
                &mut redeemer,
                AdmissionUse::ReadOnly,
                1_000,
            )
            .err()
            .unwrap(),
        HarnessTransportError::CredentialDenied
    );
    assert_eq!(redeemer.calls, 0);

    let wrong_reference = ResolvedOwnerCredential::new(
        ProtectedAuthReference::new("other-ref").unwrap(),
        credential(&invocation, &["workflow:read"], 1_900)
            .descriptor()
            .clone(),
    );
    assert_eq!(
        transport
            .redeem_for_invocation(
                &invocation,
                &wrong_reference,
                &mut redeemer,
                AdmissionUse::ReadOnly,
                1_000,
            )
            .err()
            .unwrap(),
        HarnessTransportError::CredentialDenied
    );
    assert_eq!(redeemer.calls, 0);

    let missing_scope = credential(&invocation, &["workflow:context:edit"], 1_900);
    assert_eq!(
        transport
            .redeem_for_invocation(
                &invocation,
                &missing_scope,
                &mut redeemer,
                AdmissionUse::ReadOnly,
                1_000,
            )
            .err()
            .unwrap(),
        HarnessTransportError::CredentialDenied
    );
    let expired = credential(&invocation, &["workflow:read"], 1_000);
    assert_eq!(
        transport
            .redeem_for_invocation(
                &invocation,
                &expired,
                &mut redeemer,
                AdmissionUse::ReadOnly,
                1_000,
            )
            .err()
            .unwrap(),
        HarnessTransportError::CredentialDenied
    );
    assert_eq!(redeemer.calls, 0);
}

#[test]
fn redemption_requests_exact_read_scope_and_maps_unavailable_fail_closed() {
    let invocation = source_status_invocation();
    let owner_credential = credential(&invocation, &["workflow:read"], 1_900);
    let transport = transport_for(
        SocketAddr::from(([127, 0, 0, 1], 43129)),
        Duration::from_secs(1),
    );
    let mut redeemer = TestRedeemer::default();
    let _bearer = transport
        .redeem_for_invocation(
            &invocation,
            &owner_credential,
            &mut redeemer,
            AdmissionUse::ReadOnly,
            1_000,
        )
        .unwrap();
    assert_eq!(redeemer.calls, 1);
    assert_eq!(redeemer.seen_reference.as_deref(), Some("ref-1"));
    assert_eq!(redeemer.seen_scopes, vec!["workflow:read".to_owned()]);

    let mut unavailable = TestRedeemer {
        failure: Some(CredentialRedemptionError::Unavailable),
        ..TestRedeemer::default()
    };
    assert_eq!(
        transport
            .redeem_for_invocation(
                &invocation,
                &owner_credential,
                &mut unavailable,
                AdmissionUse::ReadOnly,
                1_000,
            )
            .err()
            .unwrap(),
        HarnessTransportError::CredentialUnavailable
    );
}

#[test]
fn typed_query_scope_and_closed_endpoint_are_operation_derived() {
    let mut invocation = source_status_invocation();
    invocation.operation = ContextOwnerOperationV2::EligibleItems {
        workflow_run_id: "run-1".to_owned(),
        query: ContextOwnerItemsQueryV1 {
            draft_id: Some("draft-1".to_owned()),
            include_content: Some(true),
        },
    };
    let endpoint = invocation.endpoint().unwrap();
    assert_eq!(endpoint.path, "/v1/workflow-runs/run-1/context-owner-items");
    assert_eq!(endpoint.query[0].name, "draft_id");
    assert_eq!(endpoint.query[1].name, "include_content");
    assert_eq!(
        required_harness_scopes(&invocation.operation, AdmissionUse::ReadOnly)
            .unwrap()
            .as_slice(),
        &["workflow:read", "workflow:context:content:read"]
    );
    validate_closed_endpoint(&invocation, &endpoint).unwrap();

    let mut altered = endpoint;
    altered.query[1].value = "true&admin=true".to_owned();
    assert_eq!(
        validate_closed_endpoint(&invocation, &altered)
            .err()
            .unwrap(),
        HarnessTransportError::InvalidInvocation
    );
}

#[test]
fn request_exchange_sends_one_fixed_typed_get_and_rejects_bad_framing() {
    let reply = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
    let (address, received) = serve_once(reply.to_vec(), Duration::ZERO);
    let invocation = source_status_invocation();
    let endpoint = invocation.endpoint().unwrap();
    let transport = transport_for(address, Duration::from_secs(2));
    let result = framing::exchange(
        &transport.config,
        &endpoint,
        None,
        b"one-use-test-token",
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(result.body.as_slice(), b"ok");
    let request = received.join().unwrap();
    let request_text = std::str::from_utf8(&request).unwrap();
    assert!(
        request_text
            .starts_with("GET /v1/workflow-runs/run-1/context-owner-source-status HTTP/1.1\r\n")
    );
    assert!(request_text.contains("Authorization: Bearer one-use-test-token\r\n"));
    assert!(request_text.contains("Content-Length: 0\r\n"));
    assert!(request_text.contains("Connection: close\r\n\r\n"));
    assert!(!request_text.contains("credential_reference_id"));
    assert!(!request_text.contains("console_subject"));

    let invalid_replies = [
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\noka".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n".to_vec(),
        b"HTTP/1.1 100 Continue\r\nContent-Type: application/json\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    ];
    for (index, bad_reply) in invalid_replies.into_iter().enumerate() {
        let (bad_address, received) = serve_once(bad_reply, Duration::ZERO);
        let bad_transport = transport_for(bad_address, Duration::from_secs(2));
        let result = framing::exchange(
            &bad_transport.config,
            &endpoint,
            None,
            b"one-use-test-token",
            Instant::now() + Duration::from_secs(2),
        );
        if index == 3 {
            assert!(matches!(result, Err(framing::FrameError::ResponseTooLarge)));
        } else {
            assert!(matches!(result, Err(framing::FrameError::InvalidFraming)));
        }
        let _ = received.join().unwrap();
    }
}

#[test]
fn sealed_admission_callback_denies_before_any_socket_opens() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let invocation = source_status_invocation();
    let endpoint = invocation.endpoint().unwrap();
    let transport = transport_for(address, Duration::from_secs(1));
    let mut checked = false;
    let result = framing::exchange_with_authorization::<HarnessTransportError, _>(
        &transport.config,
        &endpoint,
        None,
        b"one-use-test-token",
        Instant::now() + Duration::from_secs(1),
        || {
            checked = true;
            Err(HarnessTransportError::CredentialDenied)
        },
    );
    assert_eq!(
        result.err().unwrap(),
        HarnessTransportError::CredentialDenied
    );
    assert!(checked);
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[test]
fn request_deadline_is_one_exchange_and_never_reconnects() {
    let (address, received) = serve_once(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_vec(),
        Duration::from_millis(120),
    );
    let invocation = source_status_invocation();
    let endpoint = invocation.endpoint().unwrap();
    let transport = transport_for(address, Duration::from_secs(1));
    let result = framing::exchange(
        &transport.config,
        &endpoint,
        None,
        b"one-use-test-token",
        Instant::now() + Duration::from_millis(30),
    );
    assert!(matches!(result, Err(framing::FrameError::Deadline)));
    let _ = received.join().unwrap();
}

#[test]
fn management_errors_are_closed_duplicate_rejecting_and_message_free() {
    let command = ContextControlCommand::Pause {
        idempotency_key: "command-1".to_owned(),
        expected_control_version: 1,
    };
    let lookup = ContextOwnerOperationV2::LookupControl {
        workflow_run_id: "run-1".to_owned(),
        command,
    };
    let no_receipt = br#"{"schema_version":"ascension.management/v1","error":{"class":"invalid_input","code":"context_control_receipt_not_recorded","message":"private upstream message"}}"#;
    assert_eq!(
        errors::management_error(404, no_receipt, &lookup),
        HarnessTransportError::ControlReceiptNotRecorded
    );
    assert!(
        !HarnessTransportError::ControlReceiptNotRecorded
            .to_string()
            .contains("private upstream message")
    );
    let duplicate = br#"{"schema_version":"ascension.management/v1","schema_version":"ascension.management/v1","error":{"class":"invalid_input","code":"context_control_receipt_not_recorded","message":"x"}}"#;
    assert_eq!(
        errors::management_error(404, duplicate, &lookup),
        HarnessTransportError::InvalidManagementError
    );
    let unknown = br#"{"schema_version":"ascension.management/v1","error":{"class":"invalid_input","code":"context_control_receipt_not_recorded","message":"x","debug":"leak"}}"#;
    assert_eq!(
        errors::management_error(404, unknown, &lookup),
        HarnessTransportError::InvalidManagementError
    );
    assert_eq!(
        errors::management_error(409, no_receipt, &lookup),
        HarnessTransportError::InvalidManagementError
    );
}

pub(super) fn serve_once(
    reply: Vec<u8>,
    delay: Duration,
) -> (SocketAddr, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = read_request(&mut stream);
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        let _ = stream.write_all(&reply);
        request
    });
    (address, handle)
}

fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let size = stream.read(&mut buffer).unwrap();
        if size == 0 {
            return request;
        }
        request.extend_from_slice(&buffer[..size]);
        let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") else {
            continue;
        };
        let header = std::str::from_utf8(&request[..header_end]).unwrap();
        let length = header
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or_default();
        if request.len() >= header_end + 4 + length {
            return request;
        }
    }
}
