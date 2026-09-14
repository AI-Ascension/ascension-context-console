// SPDX-License-Identifier: MIT

//! Producer-library synthetic contract evidence, not an attached owner or native/provider run.

use context_service::effective_limits::{
    Adoption, ConsumerPin, EffectiveLimitRecord, LimitClass, PRODUCER_REVISION, UnavailableReason,
    admit_consumer, console_consumer_pin_for,
};
use context_service::{
    AdvertisedMemoryCapabilities, AdvertisedSessionCapabilities, MemoryCapabilities,
    SessionCapabilitiesView, read_advertised_memory_capabilities,
    read_advertised_session_capabilities,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

const FIXTURE: &str = include_str!("../../../fixtures/effective-limits/producer.json");

enum Descriptor {
    Memory(Box<MemoryCapabilities>),
    Session(Box<SessionCapabilitiesView>),
}

impl Descriptor {
    fn read(surface: &str, value: &Value) -> Self {
        let bytes = serde_json::to_vec(value).expect("descriptor JSON");
        match surface {
            "memory" => match read_advertised_memory_capabilities(&bytes).expect("memory reader") {
                AdvertisedMemoryCapabilities::V3(value) => Self::Memory(value),
                _ => panic!("producer fixture must advertise v3"),
            },
            "session" => {
                match read_advertised_session_capabilities(&bytes).expect("session reader") {
                    AdvertisedSessionCapabilities::V3(value) => Self::Session(value),
                    _ => panic!("producer fixture must advertise v3"),
                }
            }
            _ => panic!("unknown test surface"),
        }
    }

    fn trusted_record(&self) -> EffectiveLimitRecord {
        // Derived from the separately pinned descriptor, never from the record under test.
        match self {
            Self::Memory(value) => value.effective_limit_record(),
            Self::Session(value) => value.effective_limit_record(),
        }
    }

    fn admit(
        &self,
        record: &EffectiveLimitRecord,
        field: &str,
        value: u64,
    ) -> Result<(), UnavailableReason> {
        match self {
            Self::Memory(descriptor) => descriptor.admit_authorized_record(record, field, value),
            Self::Session(descriptor) => descriptor.admit_authorized_record(record, field, value),
        }
    }
}

fn cases() -> Vec<(String, Value)> {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("producer fixture");
    assert_eq!(fixture["producer_revision"], PRODUCER_REVISION);
    ["memory", "session"]
        .into_iter()
        .flat_map(|surface| {
            let cases = fixture[surface].as_array().expect("surface cases");
            assert_eq!(
                cases
                    .iter()
                    .map(|case| case["name"].as_str().expect("name"))
                    .collect::<Vec<_>>(),
                ["default", "restricted", "disabled"]
            );
            cases
                .iter()
                .map(|case| (surface.to_owned(), case.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn record(case: &Value) -> EffectiveLimitRecord {
    serde_json::from_value(case["record"].clone()).expect("producer record")
}

#[test]
fn producer_records_match_real_dual_reader_derivation_and_every_admission_boundary() {
    for (surface, case) in cases() {
        let descriptor = Descriptor::read(&surface, &case["descriptor"]);
        let published = record(&case);
        assert_eq!(
            descriptor.trusted_record(),
            published,
            "{surface}/{}",
            case["name"]
        );
        assert_eq!(published.validate(), Ok(()));
        assert_eq!(
            case["producer_descriptor_valid"],
            !(surface == "session" && case["name"] == "disabled"),
            "empty-method session is not an attachable producer profile"
        );
        for row in &published.rows {
            for requested in [0, 1, row.executable_ceiling] {
                assert_eq!(
                    descriptor.admit(&published, &row.field, requested),
                    if published.enabled {
                        Ok(())
                    } else {
                        Err(UnavailableReason::Disabled)
                    },
                    "{surface}/{} {}={requested}",
                    case["name"],
                    row.field
                );
            }
            assert_eq!(
                descriptor.admit(&published, &row.field, row.executable_ceiling + 1),
                Err(if published.enabled {
                    UnavailableReason::EffectiveLimitExceeded
                } else {
                    UnavailableReason::Disabled
                }),
                "{surface}/{} {}",
                case["name"],
                row.field
            );
            if case["name"] == "restricted" {
                assert!(
                    row.executable_ceiling < row.capabilities_schema_ceiling,
                    "{}",
                    row.field
                );
            }
        }
        assert_eq!(
            descriptor.admit(&published, "not-advertised", 1),
            Err(if published.enabled {
                UnavailableReason::FieldNotAdvertised
            } else {
                UnavailableReason::Disabled
            })
        );
    }
}

#[test]
fn producer_and_consumer_schema_ceilings_match_the_pinned_artifacts() {
    for (surface, case) in cases() {
        let (policy, capability) = if surface == "memory" {
            (
                include_str!("../../../contracts/context-memory/policy.schema.json"),
                include_str!("../../../contracts/context-memory/capabilities.schema.json"),
            )
        } else {
            (
                include_str!("../../../contracts/provider-session/policy.schema.json"),
                include_str!("../../../contracts/provider-session/capabilities.schema.json"),
            )
        };
        let policy: Value = serde_json::from_str(policy).expect("policy schema");
        let capability: Value = serde_json::from_str(capability).expect("capability schema");
        let trusted = Descriptor::read(&surface, &case["descriptor"]).trusted_record();
        for row in &trusted.rows {
            let field = if row.field == "max_history_ttl_seconds" {
                "history_ttl_seconds"
            } else {
                &row.field
            };
            assert_eq!(
                policy["properties"][field]["maximum"].as_u64(),
                row.policy_schema_ceiling
            );
            assert_eq!(
                capability["properties"]["effective_limits"]["properties"][&row.field]["maximum"]
                    .as_u64(),
                Some(row.capabilities_schema_ceiling)
            );
        }
    }
}

#[test]
fn record_mutations_never_authenticate_against_the_independent_descriptor() {
    for (surface, case) in cases() {
        let descriptor = Descriptor::read(&surface, &case["descriptor"]);
        let published = record(&case);
        let check = |mutated: &EffectiveLimitRecord, reason| {
            assert_eq!(
                descriptor.admit(mutated, &published.rows[0].field, 1),
                Err(reason)
            );
        };
        for index in 0..published.rows.len() {
            for mutation in 0..6 {
                let mut changed = published.clone();
                let row = &mut changed.rows[index];
                match mutation {
                    0 => row.executable_ceiling += 1,
                    1 => row.capabilities_schema_ceiling += 1,
                    2 => {
                        row.policy_schema_ceiling = Some(row.policy_schema_ceiling.unwrap_or(0) + 1)
                    }
                    3 => row.validator.push_str("-forged"),
                    4 => row.field.push_str("-forged"),
                    _ => {
                        row.class = if row.class == LimitClass::RuntimeGuard {
                            LimitClass::ProfileSelected
                        } else {
                            LimitClass::RuntimeGuard
                        }
                    }
                }
                check(&changed, UnavailableReason::DescriptorTampered);
            }
        }
        let mut missing = published.clone();
        missing.rows.pop();
        check(&missing, UnavailableReason::DescriptorTampered);
        let mut duplicate = published.clone();
        duplicate.rows.push(duplicate.rows[0].clone());
        check(&duplicate, UnavailableReason::DescriptorTampered);
        let mut reordered = published.clone();
        reordered.rows.swap(0, 1);
        check(&reordered, UnavailableReason::DescriptorTampered);
        let mut enabled = published.clone();
        enabled.enabled = !enabled.enabled;
        check(&enabled, UnavailableReason::DescriptorTampered);
        let mut owner = published.clone();
        owner.owner = "foreign-owner".to_owned();
        check(&owner, UnavailableReason::DescriptorTampered);
        let mut schema = published.clone();
        schema.schema = "ascension.harness.effective-limits.v0".to_owned();
        check(&schema, UnavailableReason::DescriptorTampered);
        let mut stale = published.clone();
        stale.capability_descriptor_sha256 = "0".repeat(64);
        check(&stale, UnavailableReason::DescriptorStale);
        let mut foreign = published.clone();
        foreign.owner_revision.push_str("-foreign");
        check(&foreign, UnavailableReason::ProfileMismatch);
    }
}

#[test]
fn exact_producer_payload_digests_survive_consumer_reading_and_v1_remains_readable() {
    for (surface, case) in cases() {
        let descriptor = Descriptor::read(&surface, &case["descriptor"]);
        let (expected, unsigned) = match descriptor {
            Descriptor::Memory(mut descriptor) => {
                let v1 = descriptor.clone().into_v1();
                let read =
                    read_advertised_memory_capabilities(&serde_json::to_vec(&v1).expect("v1"))
                        .expect("v1 reader");
                assert!(matches!(read, AdvertisedMemoryCapabilities::V1(_)));
                assert!(read.effective_limits().is_none());
                let expected = descriptor.binding.descriptor_sha256.clone();
                descriptor.binding.descriptor_sha256.clear();
                (
                    expected,
                    serde_json::to_vec(&descriptor).expect("unsigned memory"),
                )
            }
            Descriptor::Session(mut descriptor) => {
                let v1 = descriptor.to_v1();
                let read =
                    read_advertised_session_capabilities(&serde_json::to_vec(&v1).expect("v1"))
                        .expect("v1 reader");
                assert!(matches!(read, AdvertisedSessionCapabilities::V1(_)));
                assert!(read.effective_limits().is_none());
                let expected = descriptor.binding.descriptor_sha256.clone();
                descriptor.binding.descriptor_sha256.clear();
                (
                    expected,
                    serde_json::to_vec(&descriptor).expect("unsigned session"),
                )
            }
        };
        assert_eq!(format!("{:x}", Sha256::digest(unsigned)), expected);
        for field in ["binding", "effective_limits"] {
            let mut missing = case["descriptor"].clone();
            missing.as_object_mut().expect("object").remove(field);
            let bytes = serde_json::to_vec(&missing).expect("missing JSON");
            assert!(if surface == "memory" {
                read_advertised_memory_capabilities(&bytes).is_err()
            } else {
                read_advertised_session_capabilities(&bytes).is_err()
            });
        }
    }
}

#[test]
fn rollback_consumer_pin_stays_pending_and_adoption_cannot_bypass_authentication() {
    let current = console_consumer_pin_for(context_service::CapabilityVersion::V1);
    assert_eq!(current.adoption, Adoption::Pending);
    for surface in &current.surfaces {
        assert!(!surface.effective_limits_advertised);
        assert!(
            surface
                .advertised_capability_schema
                .as_ref()
                .expect("schema")
                .ends_with(".v1")
        );
    }
    for (surface, case) in cases() {
        let trusted = Descriptor::read(&surface, &case["descriptor"]).trusted_record();
        let published = record(&case);
        let admit = |pin: &ConsumerPin, candidate: &EffectiveLimitRecord| {
            admit_consumer(
                pin,
                &trusted.surface,
                candidate,
                &trusted,
                &trusted.rows[0].field,
                1,
            )
        };
        assert_eq!(
            admit(&current, &published),
            Err(UnavailableReason::ConsumerPinNotAdopted)
        );
        let mut synthetic_aligned = current.clone();
        synthetic_aligned.adoption = Adoption::Aligned;
        for entry in &mut synthetic_aligned.surfaces {
            entry.advertised_capability_schema =
                Some(format!("ascension.{}.capabilities.v3", entry.surface));
            entry.effective_limits_advertised = true;
        }
        assert_eq!(
            admit(&synthetic_aligned, &published),
            if published.enabled {
                Ok(())
            } else {
                Err(UnavailableReason::Disabled)
            }
        );
        let mut tampered = published.clone();
        tampered.rows[0].executable_ceiling += 1;
        assert_eq!(
            admit(&synthetic_aligned, &tampered),
            Err(UnavailableReason::DescriptorTampered)
        );
        synthetic_aligned.repository = "AI-Ascension/foreign".to_owned();
        assert_eq!(
            admit(&synthetic_aligned, &published),
            Err(UnavailableReason::ConsumerNotRecorded)
        );
    }
}
