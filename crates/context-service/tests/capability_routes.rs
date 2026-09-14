// SPDX-License-Identifier: MIT

//! Actual route + injected owner-port tests. The descriptors/records come from the pinned
//! producer library; the owner here is synthetic and performs no provider/game/native effects.

use context_service::effective_limits::{
    Adoption, EffectiveLimitRecord, LimitClass, UnavailableReason, console_consumer_pin,
    console_consumer_pin_for,
};
use context_service::*;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
enum Surface {
    Memory,
    Session,
}

fn fixture(surface: Surface, name: &str) -> Value {
    let data: Value = serde_json::from_str(include_str!(
        "../../../fixtures/effective-limits/producer.json"
    ))
    .expect("fixture");
    let key = match surface {
        Surface::Memory => "memory",
        Surface::Session => "session",
    };
    data[key]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["name"] == name)
        .expect("case")
        .clone()
}

fn scope() -> OwnerScope {
    OwnerScope::new(
        "fixture-project",
        "fixture-run",
        "fixture-episode",
        "fixture-agent",
    )
    .expect("scope")
}

fn memory_scope() -> MemoryScope {
    serde_json::from_value(fixture(Surface::Memory, "default")["descriptor"]["scope"].clone())
        .expect("memory scope")
}

fn memory_descriptor(name: &str) -> MemoryCapabilities {
    serde_json::from_value(fixture(Surface::Memory, name)["descriptor"].clone())
        .expect("memory descriptor")
}

fn session_descriptor(name: &str) -> SessionCapabilitiesView {
    serde_json::from_value(fixture(Surface::Session, name)["descriptor"].clone())
        .expect("session descriptor")
}

fn publication(surface: Surface, name: &str) -> OwnerReply {
    let case = fixture(surface, name);
    let operation = match surface {
        Surface::Memory => OwnerOperation::MemoryCapabilities,
        Surface::Session => OwnerOperation::SessionCapabilities,
    };
    let receipt = OwnerReceipt::new(
        "receipt-fixture",
        "operation-fixture",
        operation,
        "harness-fixture",
        7,
        "synthetic_producer_library",
        OwnerOutcome::Accepted,
        false,
    )
    .expect("receipt");
    let record: EffectiveLimitRecord =
        serde_json::from_value(case["record"].clone()).expect("record");
    OwnerReply::new(receipt, Some(case["descriptor"].clone()))
        .expect("reply")
        .with_effective_limits(record)
        .expect("sidecar")
}

struct RecordingOwner {
    publication: Mutex<OwnerReply>,
    calls: Mutex<Vec<OwnerOperation>>,
}

impl RecordingOwner {
    fn new(reply: OwnerReply) -> Arc<Self> {
        Arc::new(Self {
            publication: Mutex::new(reply),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn query_count(&self) -> usize {
        self.calls
            .lock()
            .expect("calls")
            .iter()
            .filter(|operation| **operation == OwnerOperation::MemoryQuery)
            .count()
    }
}

impl HarnessOwner for RecordingOwner {
    fn call(&self, request: OwnerCall) -> Result<OwnerReply, OwnerPortError> {
        self.calls.lock().expect("calls").push(request.operation);
        assert_eq!(request.scope, scope());
        if request.operation == OwnerOperation::MemoryQuery {
            let receipt = OwnerReceipt::new(
                "query-receipt",
                "query-operation",
                request.operation,
                "harness-fixture",
                7,
                "synthetic_owner",
                OwnerOutcome::Accepted,
                false,
            )
            .expect("query receipt");
            return OwnerReply::new(receipt, Some(json!({
                "schema":"owner.synthetic-query.v1", "inference_calls":0, "native_calls":0, "game_effects":0
            }))).map_err(|_| OwnerPortError::MalformedResponse);
        }
        Ok(self.publication.lock().expect("publication").clone())
    }

    fn lookup_receipt(&self, _: OwnerReceiptLookup) -> Result<OwnerReply, OwnerPortError> {
        Err(OwnerPortError::UnknownReceipt)
    }
}

fn context() -> OwnerRequestContext {
    OwnerRequestContext::new(
        "read-grant",
        "console.test",
        Some("https://console.test".to_owned()),
        None,
        1,
    )
}

fn composition(owner: Arc<RecordingOwner>) -> HarnessOwnerComposition {
    let grant = OwnerGrant::new(
        "read-grant",
        OwnerGrantClass::ReadSearch,
        scope(),
        100,
        "console.test",
        Some("https://console.test".to_owned()),
        None,
    )
    .expect("grant");
    HarnessOwnerComposition::new(owner, OwnerGrantBook::new([grant]))
}

fn trusted_composition(
    surface: Surface,
    name: &str,
    owner: Arc<RecordingOwner>,
) -> HarnessOwnerComposition {
    // This configuration is loaded independently, not copied from the owner's mutable publication.
    match surface {
        Surface::Memory => composition(owner).with_memory_capability_trust(
            MemoryCapabilityTrust::new(memory_descriptor(name), "harness-fixture", 7)
                .expect("independent memory trust"),
        ),
        Surface::Session => composition(owner).with_session_capability_trust(
            SessionCapabilityTrust::new(scope(), session_descriptor(name), "harness-fixture", 7)
                .expect("independent session trust"),
        ),
    }
}

fn get(
    surface: Surface,
    composition: HarnessOwnerComposition,
    version: CapabilityVersion,
) -> Result<Value, UnavailableReason> {
    match surface {
        Surface::Memory => match MemoryRoute::attached(memory_scope(), composition)
            .with_capability_version(version)
            .handle_with_context("GET", "/v3/memory/capabilities", &context(), &[])
        {
            Ok(value) => Ok(value),
            Err(MemoryRouteError::EffectiveLimit(reason)) => Err(reason),
            Err(error) => panic!("unexpected memory failure {error:?}"),
        },
        Surface::Session => match ProviderSessionRoute::attached_with_scope(
            "fixture",
            SessionScopeView {
                project_id: scope().project_id,
                run_id: scope().run_id,
                episode_id: scope().episode_id,
                agent_id: scope().agent_id,
            },
            composition,
        )
        .with_capability_version(version)
        .handle_with_context(
            "GET",
            "/v1/runs/fixture-run/provider-sessions/capabilities",
            &context(),
            &[],
        ) {
            Ok(value) => Ok(value),
            Err(SessionApiError::EffectiveLimit(reason)) => Err(reason),
            Err(error) => panic!("unexpected session failure {error:?}"),
        },
    }
}

#[test]
fn real_routes_consume_the_supplied_producer_record_before_presenting_v3() {
    for surface in [Surface::Memory, Surface::Session] {
        for name in ["default", "restricted"] {
            let owner = RecordingOwner::new(publication(surface, name));
            let result = get(
                surface,
                trusted_composition(surface, name, owner.clone()),
                CapabilityVersion::V3,
            )
            .expect("admitted");
            assert_eq!(result["value"], fixture(surface, name)["descriptor"]);
            assert_eq!(result["receipt"]["evidence"], "synthetic_producer_library");
            assert_eq!(result["receipt"]["effect_applied"], false);
            assert_eq!(owner.calls.lock().expect("calls").len(), 1);
            assert!(
                result.get("effective_limit_record").is_none(),
                "no new wire field"
            );
        }
    }
}

#[test]
fn route_rejects_each_limit_class_ceiling_owner_enabled_and_unknown_field_mutation() {
    for surface in [Surface::Memory, Surface::Session] {
        let original = publication(surface, "restricted");
        for row_index in 0..original
            .effective_limits
            .as_ref()
            .expect("record")
            .rows
            .len()
        {
            for mutation in 0..5 {
                let mut reply = original.clone();
                let row = &mut reply.effective_limits.as_mut().expect("record").rows[row_index];
                match mutation {
                    0 => row.executable_ceiling += 1,
                    1 => row.capabilities_schema_ceiling += 1,
                    2 => {
                        row.policy_schema_ceiling = Some(row.policy_schema_ceiling.unwrap_or(0) + 1)
                    }
                    3 => {
                        row.class = if row.class == LimitClass::RuntimeGuard {
                            LimitClass::ProfileSelected
                        } else {
                            LimitClass::RuntimeGuard
                        }
                    }
                    _ => row.field = "unknown-field".to_owned(),
                }
                let owner = RecordingOwner::new(reply);
                assert_eq!(
                    get(
                        surface,
                        trusted_composition(surface, "restricted", owner),
                        CapabilityVersion::V3
                    ),
                    Err(UnavailableReason::DescriptorTampered)
                );
            }
        }
        for mutate in [0, 1] {
            let mut reply = original.clone();
            let record = reply.effective_limits.as_mut().expect("record");
            if mutate == 0 {
                record.owner = "foreign-owner".to_owned();
            } else {
                record.enabled = !record.enabled;
            }
            let owner = RecordingOwner::new(reply);
            assert_eq!(
                get(
                    surface,
                    trusted_composition(surface, "restricted", owner),
                    CapabilityVersion::V3
                ),
                Err(UnavailableReason::DescriptorTampered)
            );
        }
    }
}

#[test]
fn route_failure_reasons_distinguish_missing_trust_record_pending_profile_and_stale_owner() {
    for surface in [Surface::Memory, Surface::Session] {
        let owner = RecordingOwner::new(publication(surface, "restricted"));
        assert_eq!(
            get(surface, composition(owner), CapabilityVersion::V3),
            Err(UnavailableReason::ConsumerNotRecorded)
        );
        let mut missing = publication(surface, "restricted");
        missing.effective_limits = None;
        let owner = RecordingOwner::new(missing);
        assert_eq!(
            get(
                surface,
                trusted_composition(surface, "restricted", owner),
                CapabilityVersion::V3
            ),
            Err(UnavailableReason::FieldNotAdvertised)
        );
        let mut legacy = publication(surface, "restricted");
        legacy.value = Some(match surface {
            Surface::Memory => {
                serde_json::to_value(memory_descriptor("restricted").into_v1()).expect("v1")
            }
            Surface::Session => {
                serde_json::to_value(session_descriptor("restricted").to_v1()).expect("v1")
            }
        });
        let owner = RecordingOwner::new(legacy);
        assert_eq!(
            get(
                surface,
                trusted_composition(surface, "restricted", owner),
                CapabilityVersion::V3
            ),
            Err(UnavailableReason::ConsumerPinNotAdopted)
        );
        for (change_source, expected) in [
            (true, UnavailableReason::ProfileMismatch),
            (false, UnavailableReason::DescriptorStale),
        ] {
            let mut reply = publication(surface, "restricted");
            if change_source {
                reply.receipt.source = "other-owner".to_owned();
            } else {
                reply.receipt.owner_epoch += 1;
            }
            let owner = RecordingOwner::new(reply);
            assert_eq!(
                get(
                    surface,
                    trusted_composition(surface, "restricted", owner),
                    CapabilityVersion::V3
                ),
                Err(expected)
            );
        }
        let mut excessive = publication(surface, "restricted");
        excessive.value.as_mut().expect("descriptor")["effective_limits"]["max_candidates"] =
            json!(64);
        let owner = RecordingOwner::new(excessive);
        assert_eq!(
            get(
                surface,
                trusted_composition(surface, "restricted", owner),
                CapabilityVersion::V3
            ),
            Err(UnavailableReason::EffectiveLimitExceeded)
        );
        let mut unknown = publication(surface, "restricted");
        unknown.value.as_mut().expect("descriptor")["unknown_field"] = json!(true);
        let owner = RecordingOwner::new(unknown);
        assert_eq!(
            get(
                surface,
                trusted_composition(surface, "restricted", owner),
                CapabilityVersion::V3
            ),
            Err(UnavailableReason::DescriptorTampered)
        );
    }
}

#[test]
fn independently_configured_scope_and_descriptor_integrity_are_required() {
    let owner = RecordingOwner::new(publication(Surface::Session, "restricted"));
    let mut foreign_scope = scope();
    foreign_scope.agent_id = "different-agent".to_owned();
    let configured = composition(owner).with_session_capability_trust(
        SessionCapabilityTrust::new(
            foreign_scope,
            session_descriptor("restricted"),
            "harness-fixture",
            7,
        )
        .expect("scoped trust"),
    );
    assert_eq!(
        get(Surface::Session, configured, CapabilityVersion::V3),
        Err(UnavailableReason::ProfileMismatch)
    );
    let mut tampered = memory_descriptor("restricted");
    tampered.effective_limits.max_candidates += 1;
    assert!(matches!(
        MemoryCapabilityTrust::new(tampered, "harness-fixture", 7),
        Err(UnavailableReason::DescriptorTampered)
    ));
    assert!(
        matches!(
            SessionCapabilityTrust::new(
                scope(),
                session_descriptor("disabled"),
                "harness-fixture",
                7
            ),
            Err(UnavailableReason::DescriptorTampered)
        ),
        "empty methods never become an attachable native profile"
    );
}

fn query(limit: usize, candidates: usize, text: String) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema":"ascension.context-memory.query.v1", "scope":memory_scope(), "branch_id":"branch-fixture",
        "query":text, "cutoff":1, "corpus_generation":1, "ranker_version":"lexical-v1",
        "limit":limit, "max_candidates":candidates, "effect_class":"local_read_no_inference"
    })).expect("query")
}

#[test]
fn memory_query_uses_fresh_supplied_record_and_never_dispatches_over_selected_limits() {
    for (limit, candidates, text, expected) in [
        (4, 8, "visible context".to_owned(), None),
        (
            5,
            8,
            "visible context".to_owned(),
            Some(UnavailableReason::EffectiveLimitExceeded),
        ),
        (
            4,
            9,
            "visible context".to_owned(),
            Some(UnavailableReason::EffectiveLimitExceeded),
        ),
        (
            4,
            8,
            "x".repeat(1025),
            Some(UnavailableReason::EffectiveLimitExceeded),
        ),
    ] {
        let owner = RecordingOwner::new(publication(Surface::Memory, "restricted"));
        let route = MemoryRoute::attached(
            memory_scope(),
            trusted_composition(Surface::Memory, "restricted", owner.clone()),
        );
        let result = route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &context(),
            &query(limit, candidates, text),
        );
        if let Some(reason) = expected {
            assert_eq!(result, Err(MemoryRouteError::EffectiveLimit(reason)));
            assert_eq!(owner.query_count(), 0);
            assert_eq!(
                owner.calls.lock().expect("calls").as_slice(),
                [OwnerOperation::MemoryCapabilities]
            );
        } else {
            let result = result.expect("accepted query");
            assert_eq!(owner.query_count(), 1);
            assert_eq!(result["value"]["inference_calls"], 0);
            assert_eq!(result["value"]["game_effects"], 0);
            assert_eq!(result["value"]["native_calls"], 0);
        }
    }
}

#[test]
fn disabled_memory_remains_discoverable_but_query_returns_machine_readable_disabled() {
    let owner = RecordingOwner::new(publication(Surface::Memory, "disabled"));
    let configured = trusted_composition(Surface::Memory, "disabled", owner.clone());
    let result =
        get(Surface::Memory, configured.clone(), CapabilityVersion::V3).expect("disabled metadata");
    assert_eq!(result["value"]["enabled"], false);
    let route = MemoryRoute::attached(memory_scope(), configured);
    let error = route
        .handle_with_context(
            "POST",
            "/v3/memory/search",
            &context(),
            &query(1, 1, "query".to_owned()),
        )
        .expect_err("disabled");
    assert_eq!(error.to_string(), "disabled");
    assert_eq!(owner.query_count(), 0);
}

#[test]
fn v1_rollback_omits_limits_and_preserves_reader_compatibility_without_claiming_adoption() {
    assert_eq!(console_consumer_pin().adoption, Adoption::Aligned);
    let rollback = console_consumer_pin_for(CapabilityVersion::V1);
    assert_eq!(rollback.adoption, Adoption::Pending);
    assert!(
        rollback
            .surfaces
            .iter()
            .all(|surface| !surface.effective_limits_advertised)
    );
    for surface in [Surface::Memory, Surface::Session] {
        let mut reply = publication(surface, "default");
        reply.effective_limits = None;
        let owner = RecordingOwner::new(reply);
        let result = get(surface, composition(owner), CapabilityVersion::V1).expect("rollback");
        assert!(
            result["value"]["schema"]
                .as_str()
                .expect("schema")
                .ends_with(".v1")
        );
        assert!(result["value"].get("binding").is_none());
        assert!(result["value"].get("effective_limits").is_none());
    }
}

#[test]
fn unauthorized_origin_fails_before_any_owner_capability_or_query_read() {
    let owner = RecordingOwner::new(publication(Surface::Memory, "restricted"));
    let route = MemoryRoute::attached(
        memory_scope(),
        trusted_composition(Surface::Memory, "restricted", owner.clone()),
    );
    let mut forged = context();
    forged.origin = Some("https://foreign.test".to_owned());
    assert_eq!(
        route.handle_with_context("GET", "/v3/memory/capabilities", &forged, &[]),
        Err(MemoryRouteError::PermissionDenied)
    );
    assert_eq!(
        route.handle_with_context(
            "POST",
            "/v3/memory/search",
            &forged,
            &query(1, 1, "query".to_owned())
        ),
        Err(MemoryRouteError::PermissionDenied)
    );
    assert!(owner.calls.lock().expect("calls").is_empty());
}
