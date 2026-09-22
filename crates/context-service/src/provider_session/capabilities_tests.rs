// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn served_capability_is_valid_v3_with_explicit_v1_rollback() {
    let mut route = ProviderSessionRoute::fixture("operator");
    let served = route
        .handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions/capabilities",
            "operator",
            &[],
        )
        .expect("capabilities");
    assert_eq!(served["value"]["schema"], SESSION_CAPABILITIES_SCHEMA);
    route
        .capabilities()
        .validate_descriptor()
        .expect("valid descriptor");
    let mut route = route.with_capability_version(crate::CapabilityVersion::V1);
    let served = route
        .handle(
            "GET",
            "/v1/runs/run-fixture/provider-sessions/capabilities",
            "operator",
            &[],
        )
        .expect("rollback");
    assert_eq!(served["value"]["schema"], SESSION_CAPABILITIES_SCHEMA_V1);
    assert!(served["value"].get("effective_limits").is_none());
    assert!(served["value"].get("binding").is_none());
}

#[test]
fn v3_target_capability_advertises_effective_limits_and_binding() {
    let route = ProviderSessionRoute::fixture("operator");
    let value = serde_json::to_value(route.target_capabilities_v3()).expect("capabilities");
    assert_eq!(value["schema"], SESSION_CAPABILITIES_SCHEMA);
    assert_eq!(
        value["effective_limits"]["policy_schema"],
        "ascension.provider-session.policy.v1"
    );
    assert_eq!(value["effective_limits"]["max_completed_turns"], 128);
    assert_eq!(value["binding"]["owner"], "sts2-harness");
    assert_eq!(
        value["binding"]["owner_revision"],
        "harness-provider-session-v3"
    );
    assert_eq!(
        value["binding"]["descriptor_sha256"],
        route
            .target_capabilities_v3()
            .descriptor_digest()
            .expect("payload digest")
    );
}

#[test]
fn dual_reader_reads_legacy_and_current_payloads() {
    let route = ProviderSessionRoute::fixture("operator");
    let v3_bytes = serde_json::to_vec(route.target_capabilities_v3()).expect("v3 bytes");
    let v3 = read_advertised_session_capabilities(&v3_bytes).expect("v3 reads");
    assert_eq!(v3.schema(), SESSION_CAPABILITIES_SCHEMA);
    // Slash-containing method names are admitted by the widened v3 pattern.
    let value: Value = serde_json::from_slice(&v3_bytes).expect("value");
    assert!(
        value["enabled_methods"]
            .as_array()
            .expect("methods")
            .iter()
            .any(|method| method == "thread/read")
    );
    assert_eq!(
        v3.effective_limits().expect("limits").max_completed_turns,
        128
    );

    let mut legacy = serde_json::to_value(route.target_capabilities_v3()).expect("capabilities");
    let object = legacy.as_object_mut().expect("object");
    object.remove("effective_limits");
    object.remove("binding");
    object.insert(
        "schema".to_owned(),
        Value::String(SESSION_CAPABILITIES_SCHEMA_V1.to_owned()),
    );
    let v1_bytes = serde_json::to_vec(&legacy).expect("v1 bytes");
    let v1 = read_advertised_session_capabilities(&v1_bytes).expect("v1 reads");
    assert_eq!(v1.schema(), SESSION_CAPABILITIES_SCHEMA_V1);
    assert!(v1.effective_limits().is_none());
    // A v1 payload cannot advertise the executable ceiling.
    assert_eq!(
        read_advertised_session_capabilities(b"not json"),
        Err(SessionCapabilitiesReadError::Malformed)
    );
}

#[test]
fn session_admission_fails_closed_and_authenticates_record() {
    let route = ProviderSessionRoute::fixture("operator");
    assert_eq!(route.admit_policy_value("max_completed_turns", 128), Ok(()));
    assert_eq!(
        route.admit_policy_value("max_completed_turns", 129),
        Err(UnavailableReason::EffectiveLimitExceeded)
    );
    assert_eq!(
        route.admit_policy_value("absent", 1),
        Err(UnavailableReason::FieldNotAdvertised)
    );

    let trusted = route.effective_limit_record();
    let mut tampered = trusted.clone();
    tampered.rows[0].policy_schema_ceiling = Some(128);
    assert_eq!(
        route.admit_authorized_record(&tampered, "max_completed_turns", 128),
        Err(UnavailableReason::DescriptorTampered)
    );
}
