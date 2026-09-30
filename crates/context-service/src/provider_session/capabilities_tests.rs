// SPDX-License-Identifier: MIT

use super::*;

#[test]
fn served_capability_is_valid_v4_with_explicit_v1_rollback() {
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
fn v4_target_capability_advertises_effective_limits_and_binding() {
    let route = ProviderSessionRoute::fixture("operator");
    let value = serde_json::to_value(route.target_capabilities()).expect("capabilities");
    assert_eq!(value["schema"], SESSION_CAPABILITIES_SCHEMA);
    assert_eq!(
        value["effective_limits"]["policy_schema"],
        "ascension.provider-session.policy.v1"
    );
    assert_eq!(value["effective_limits"]["max_completed_turns"], 128);
    assert_eq!(value["binding"]["owner"], "sts2-harness");
    assert_eq!(
        value["binding"]["owner_revision"],
        "harness-provider-session-v4"
    );
    assert_eq!(
        value["binding"]["descriptor_sha256"],
        route
            .target_capabilities()
            .descriptor_digest()
            .expect("payload digest")
    );
}

#[test]
fn dual_reader_reads_legacy_and_current_payloads() {
    let route = ProviderSessionRoute::fixture("operator");
    let v4_bytes = serde_json::to_vec(route.target_capabilities()).expect("v4 bytes");
    let v4 = read_advertised_session_capabilities(&v4_bytes).expect("v4 reads");
    assert_eq!(v4.schema(), SESSION_CAPABILITIES_SCHEMA);
    // Slash-containing method names are admitted by the widened pattern.
    let value: Value = serde_json::from_slice(&v4_bytes).expect("value");
    assert!(
        value["enabled_methods"]
            .as_array()
            .expect("methods")
            .iter()
            .any(|method| method == "thread/read")
    );
    assert_eq!(
        v4.effective_limits().expect("limits").max_completed_turns,
        128
    );

    let mut legacy = serde_json::to_value(route.target_capabilities()).expect("capabilities");
    let object = legacy.as_object_mut().expect("object");
    object.remove("effective_limits");
    object.remove("binding");
    // `v1` has no executable-limit objects and still names the qualifier `evidence`; the `v4`
    // descriptor names it `provenance`. The synthesized payload has to carry the `v1` spelling.
    let provenance = object.remove("provenance").expect("provenance");
    object.insert("evidence".to_owned(), provenance);
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

/// The `v4` rename is wire-visible: `provenance` is required by the v4 contract and `evidence` is
/// refused, because both schemas set `additionalProperties: false`. This test is the in-repo proof
/// that a `v3`-shaped payload is not silently accepted as `v4` (and vice versa) merely because the
/// structs are otherwise identical.
#[test]
fn qualifier_rename_is_wire_visible_between_v3_and_v4() {
    let route = ProviderSessionRoute::fixture("operator");
    let mut v4_value = serde_json::to_value(route.target_capabilities()).expect("v4 value");

    // A v4 payload keeps `provenance` and does not carry the v3 `evidence` name.
    let object = v4_value.as_object_mut().expect("object");
    let provenance = object
        .remove("provenance")
        .expect("v4 advertises provenance");
    assert!(object.get("evidence").is_none());

    // Re-inserting the v3 name under a v4 `schema` is refused, not coerced.
    let mut mislabelled = v4_value.clone();
    mislabelled
        .as_object_mut()
        .expect("object")
        .insert("evidence".to_owned(), provenance.clone());
    assert_eq!(
        read_advertised_session_capabilities(&serde_json::to_vec(&mislabelled).expect("bytes")),
        Err(SessionCapabilitiesReadError::Malformed)
    );

    // The same payload under the v3 `schema`, carrying `evidence`, is still readable (ADR 0020
    // decision 4: do not remove v3 reading during migration).
    let mut v3_value = v4_value.clone();
    let object = v3_value.as_object_mut().expect("object");
    object.insert("evidence".to_owned(), provenance);
    object.insert(
        "schema".to_owned(),
        Value::String(SESSION_CAPABILITIES_SCHEMA_V3.to_owned()),
    );
    let v3_bytes = serde_json::to_vec(&v3_value).expect("v3 bytes");
    let v3 = read_advertised_session_capabilities(&v3_bytes).expect("v3 still reads");
    assert_eq!(v3.schema(), SESSION_CAPABILITIES_SCHEMA_V3);
    assert_eq!(
        v3.effective_limits().expect("limits").max_completed_turns,
        128
    );

    // A `v3`-shaped payload relabelled as `v4` is refused: `v4` requires `provenance`, and the
    // reader never coerces the old name into the new one.
    let mut relabelled = v3_value.clone();
    relabelled.as_object_mut().expect("object").insert(
        "schema".to_owned(),
        Value::String(SESSION_CAPABILITIES_SCHEMA.to_owned()),
    );
    assert_eq!(
        read_advertised_session_capabilities(&serde_json::to_vec(&relabelled).expect("bytes")),
        Err(SessionCapabilitiesReadError::Malformed)
    );
}

/// A `v3` and a `v4` descriptor of the same profile must derive the same effective-limit record:
/// the rename touches neither the ceilings nor the record identity.
#[test]
fn v3_and_v4_derive_the_same_effective_limit_record() {
    let route = ProviderSessionRoute::fixture("operator");
    let v4_record = route.target_capabilities().effective_limit_record();

    let mut v3_value = serde_json::to_value(route.target_capabilities()).expect("v4 value");
    let object = v3_value.as_object_mut().expect("object");
    let provenance = object.remove("provenance").expect("provenance");
    object.insert("evidence".to_owned(), provenance);
    object.insert(
        "schema".to_owned(),
        Value::String(SESSION_CAPABILITIES_SCHEMA_V3.to_owned()),
    );
    let v3_bytes = serde_json::to_vec(&v3_value).expect("v3 bytes");
    let AdvertisedSessionCapabilities::V3(v3) =
        read_advertised_session_capabilities(&v3_bytes).expect("v3 reads")
    else {
        panic!("payload advertises the v3 schema");
    };
    let v3_record = v3.effective_limit_record();

    // The ceiling rows are identical; only the advertised schema string differs.
    assert_eq!(v3_record.rows, v4_record.rows);
    assert_eq!(v3_record.surface, v4_record.surface);
    assert_eq!(v3_record.owner, v4_record.owner);
    assert_eq!(v3_record.enabled, v4_record.enabled);
    assert_eq!(v3_record.capability_schema, SESSION_CAPABILITIES_SCHEMA_V3);
    assert_eq!(v4_record.capability_schema, SESSION_CAPABILITIES_SCHEMA);
}
