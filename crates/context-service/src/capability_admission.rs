// SPDX-License-Identifier: MIT

//! Internal owner-port admission. Trust is selected at composition time, independently of the
//! owner reply and its record. No transport or owner-record derivation is implemented here.

use crate::effective_limits::{UnavailableReason, admit_consumer, console_consumer_pin};
use crate::owner::{OwnerOperation, OwnerOutcome, OwnerReply, OwnerScope};
use crate::{
    AdvertisedMemoryCapabilities, AdvertisedSessionCapabilities, CapabilityVersion,
    MemoryCapabilities, SessionCapabilitiesView, read_advertised_memory_capabilities,
    read_advertised_session_capabilities,
};
use serde_json::Value;

#[derive(Clone, Debug)]
struct OwnerBinding {
    scope: OwnerScope,
    source: String,
    epoch: u64,
}

impl OwnerBinding {
    fn new(scope: OwnerScope, source: String, epoch: u64) -> Result<Self, UnavailableReason> {
        if !scope.is_valid() || source.is_empty() || source.len() > 128 || epoch == 0 {
            return Err(UnavailableReason::ProfileMismatch);
        }
        Ok(Self {
            scope,
            source,
            epoch,
        })
    }

    fn check(&self, scope: &OwnerScope, reply: &OwnerReply) -> Result<(), UnavailableReason> {
        if scope != &self.scope || reply.receipt.source != self.source {
            return Err(UnavailableReason::ProfileMismatch);
        }
        if reply.receipt.owner_epoch != self.epoch {
            return Err(UnavailableReason::DescriptorStale);
        }
        Ok(())
    }
}

/// Independently configured owner/scope/revision and descriptor for the memory capability port.
/// Never construct this from the reply whose limits are about to be presented.
#[derive(Clone, Debug)]
pub struct MemoryCapabilityTrust {
    owner: OwnerBinding,
    descriptor: MemoryCapabilities,
}

impl MemoryCapabilityTrust {
    pub fn new(
        descriptor: MemoryCapabilities,
        owner_source: impl Into<String>,
        owner_epoch: u64,
    ) -> Result<Self, UnavailableReason> {
        descriptor.validate_descriptor()?;
        let scope = crate::owner::owner_scope(
            &descriptor.scope.project_id,
            &descriptor.scope.run_id,
            &descriptor.scope.episode_id,
            &descriptor.scope.agent_id,
        );
        Ok(Self {
            owner: OwnerBinding::new(scope, owner_source.into(), owner_epoch)?,
            descriptor,
        })
    }

    fn present(&self, scope: &OwnerScope, reply: &mut OwnerReply) -> Result<(), UnavailableReason> {
        self.owner.check(scope, reply)?;
        let record = reply
            .effective_limits
            .as_ref()
            .ok_or(UnavailableReason::FieldNotAdvertised)?;
        let trusted = self.descriptor.effective_limit_record();
        record.authenticate(&trusted)?;
        let value = reply
            .value
            .as_ref()
            .ok_or(UnavailableReason::FieldNotAdvertised)?;
        let bytes = serde_json::to_vec(value).map_err(|_| UnavailableReason::DescriptorTampered)?;
        let AdvertisedMemoryCapabilities::V3(descriptor) =
            read_advertised_memory_capabilities(&bytes)
                .map_err(|_| UnavailableReason::DescriptorTampered)?
        else {
            return Err(UnavailableReason::ConsumerPinNotAdopted);
        };
        if descriptor.scope != self.descriptor.scope {
            return Err(UnavailableReason::ProfileMismatch);
        }
        if trusted.enabled {
            for row in descriptor.effective_limit_record().rows {
                admit_consumer(
                    &console_consumer_pin(),
                    "context-memory",
                    record,
                    &trusted,
                    &row.field,
                    row.executable_ceiling,
                )?;
            }
        }
        descriptor.validate_descriptor()?;
        if *descriptor != self.descriptor {
            return Err(UnavailableReason::DescriptorStale);
        }
        // Disabled capabilities remain discoverable metadata; enabled=false never admits a value.
        reply.value = Some(
            serde_json::to_value(descriptor).map_err(|_| UnavailableReason::DescriptorTampered)?,
        );
        Ok(())
    }
}

/// Independently configured selected session profile, owner epoch and scope.
#[derive(Clone, Debug)]
pub struct SessionCapabilityTrust {
    owner: OwnerBinding,
    descriptor: SessionCapabilitiesView,
}

impl SessionCapabilityTrust {
    pub fn new(
        scope: OwnerScope,
        descriptor: SessionCapabilitiesView,
        owner_source: impl Into<String>,
        owner_epoch: u64,
    ) -> Result<Self, UnavailableReason> {
        descriptor.validate_descriptor()?;
        Ok(Self {
            owner: OwnerBinding::new(scope, owner_source.into(), owner_epoch)?,
            descriptor,
        })
    }

    fn present(&self, scope: &OwnerScope, reply: &mut OwnerReply) -> Result<(), UnavailableReason> {
        self.owner.check(scope, reply)?;
        let record = reply
            .effective_limits
            .as_ref()
            .ok_or(UnavailableReason::FieldNotAdvertised)?;
        let trusted = self.descriptor.effective_limit_record();
        record.authenticate(&trusted)?;
        let value = reply
            .value
            .as_ref()
            .ok_or(UnavailableReason::FieldNotAdvertised)?;
        let bytes = serde_json::to_vec(value).map_err(|_| UnavailableReason::DescriptorTampered)?;
        let AdvertisedSessionCapabilities::V3(descriptor) =
            read_advertised_session_capabilities(&bytes)
                .map_err(|_| UnavailableReason::DescriptorTampered)?
        else {
            return Err(UnavailableReason::ConsumerPinNotAdopted);
        };
        for row in descriptor.effective_limit_record().rows {
            admit_consumer(
                &console_consumer_pin(),
                "provider-session",
                record,
                &trusted,
                &row.field,
                row.executable_ceiling,
            )?;
        }
        descriptor.validate_descriptor()?;
        if *descriptor != self.descriptor {
            return Err(UnavailableReason::DescriptorStale);
        }
        reply.value = Some(
            serde_json::to_value(descriptor).map_err(|_| UnavailableReason::DescriptorTampered)?,
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct OwnerCapabilityTrust {
    pub memory: Option<MemoryCapabilityTrust>,
    pub session: Option<SessionCapabilityTrust>,
}

impl OwnerCapabilityTrust {
    pub fn admit_memory_query(
        &self,
        scope: &OwnerScope,
        reply: &OwnerReply,
        request: &crate::MemoryQueryRequest,
    ) -> Result<(), UnavailableReason> {
        let configured = self
            .memory
            .as_ref()
            .ok_or(UnavailableReason::ConsumerNotRecorded)?;
        configured.owner.check(scope, reply)?;
        let record = reply
            .effective_limits
            .as_ref()
            .ok_or(UnavailableReason::FieldNotAdvertised)?;
        let trusted = configured.descriptor.effective_limit_record();
        for (field, requested) in [
            ("max_results", request.limit),
            ("max_candidates", request.max_candidates),
            ("max_query_bytes", request.query.len()),
        ] {
            admit_consumer(
                &console_consumer_pin(),
                "context-memory",
                record,
                &trusted,
                field,
                requested as u64,
            )?;
        }
        Ok(())
    }

    pub fn present(
        &self,
        operation: OwnerOperation,
        scope: &OwnerScope,
        reply: &mut OwnerReply,
        version: CapabilityVersion,
    ) -> Result<(), UnavailableReason> {
        if reply.receipt.outcome != OwnerOutcome::Accepted {
            return Ok(());
        }
        match (version, operation) {
            (CapabilityVersion::V3, OwnerOperation::MemoryCapabilities) => self
                .memory
                .as_ref()
                .ok_or(UnavailableReason::ConsumerNotRecorded)?
                .present(scope, reply),
            (CapabilityVersion::V3, OwnerOperation::SessionCapabilities) => self
                .session
                .as_ref()
                .ok_or(UnavailableReason::ConsumerNotRecorded)?
                .present(scope, reply),
            (
                CapabilityVersion::V1,
                OwnerOperation::MemoryCapabilities | OwnerOperation::SessionCapabilities,
            ) => {
                reply.value = Some(legacy_projection(
                    operation,
                    reply
                        .value
                        .as_ref()
                        .ok_or(UnavailableReason::FieldNotAdvertised)?,
                )?);
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

fn legacy_projection(operation: OwnerOperation, value: &Value) -> Result<Value, UnavailableReason> {
    let bytes = serde_json::to_vec(value).map_err(|_| UnavailableReason::DescriptorTampered)?;
    if operation == OwnerOperation::MemoryCapabilities {
        match read_advertised_memory_capabilities(&bytes)
            .map_err(|_| UnavailableReason::DescriptorTampered)?
        {
            AdvertisedMemoryCapabilities::V1(descriptor) => serde_json::to_value(descriptor),
            AdvertisedMemoryCapabilities::V3(descriptor) => {
                descriptor.validate_descriptor()?;
                serde_json::to_value(descriptor.into_v1())
            }
        }
    } else {
        match read_advertised_session_capabilities(&bytes)
            .map_err(|_| UnavailableReason::DescriptorTampered)?
        {
            AdvertisedSessionCapabilities::V1(descriptor) => serde_json::to_value(descriptor),
            AdvertisedSessionCapabilities::V3(descriptor) => {
                descriptor.validate_descriptor()?;
                serde_json::to_value(descriptor.to_v1())
            }
        }
    }
    .map_err(|_| UnavailableReason::DescriptorTampered)
}
