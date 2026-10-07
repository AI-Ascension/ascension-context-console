use super::super::boundary::ContextOwnerBinding;
use super::super::source::ContextSourceAdoptionRequest;
use super::super::validation::{
    OwnerWireError, validate_digest, validate_identifier, validate_schema,
};
use super::{
    CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2, ContextControlCommandKind, ContextControlReceipt,
};

impl ContextControlReceipt {
    /// Validates every transition field that the current closed adoption request can witness.
    /// The Harness receipt does not echo its source ID, and the request does not carry the
    /// advertised source digest. A structurally valid receipt is therefore refused until the
    /// invocation supplies that immutable source witness; matching two receipt digests alone
    /// cannot prove which source was adopted.
    pub fn validate_adoption_for(
        &self,
        binding: &ContextOwnerBinding,
        request: &ContextSourceAdoptionRequest,
    ) -> Result<(), OwnerWireError> {
        validate_schema(
            &self.schema_version,
            CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2,
        )?;
        binding.validate()?;
        request.validate()?;
        if request.expected_control_version != binding.boundary.control_version
            || request.expected_boundary != binding.boundary
            || request.expected_revision_id != binding.approved_revision_id
        {
            return Err(OwnerWireError::CorrelationMismatch("adoption_precondition"));
        }

        for (field, value) in [
            ("adoption_receipt_owner_id", self.owner_id.as_str()),
            (
                "adoption_receipt_invocation_id",
                self.invocation_id.as_str(),
            ),
            ("adoption_receipt_binding_id", self.binding_id.as_str()),
            ("adoption_receipt_command_id", self.command_id.as_str()),
            (
                "adoption_receipt_idempotency_key",
                self.idempotency_key.as_str(),
            ),
            ("adoption_receipt_effect", self.effect.as_str()),
        ] {
            validate_identifier(field, value)?;
        }
        validate_digest("adoption_receipt_binding_digest", &self.binding_digest)?;

        let expected_control_version = request
            .expected_boundary
            .control_version
            .checked_add(1)
            .ok_or(OwnerWireError::OutOfBounds("control_version"))?;
        let expected_plan_epoch = binding
            .plan_epoch
            .checked_add(1)
            .ok_or(OwnerWireError::OutOfBounds("plan_epoch"))?;
        let expected_revision_id = format!("revision-{expected_plan_epoch}");
        let mut expected_boundary = request.expected_boundary.clone();
        expected_boundary.control_version = expected_control_version;
        expected_boundary.validate()?;

        if self.owner_id != binding.owner_id
            || self.invocation_id != binding.invocation_id
            || self.binding_id != binding.binding_id
            || self.binding_digest != binding.binding_digest
            || self.command != ContextControlCommandKind::Commit
            || self.idempotency_key != request.idempotency_key
            || self.effect != "revision_committed"
            || self.control_version != expected_control_version
            || self.plan_epoch != expected_plan_epoch
            || self.boundary != expected_boundary
            || self.controller_epoch != expected_boundary.controller_epoch
            || self.gate_epoch != expected_boundary.gate_epoch
        {
            return Err(OwnerWireError::CorrelationMismatch("adoption_receipt"));
        }

        let revision_id = self
            .revision_id
            .as_deref()
            .ok_or(OwnerWireError::CorrelationMismatch("adoption_revision"))?;
        validate_identifier("adoption_result_revision_id", revision_id)?;
        if revision_id != expected_revision_id {
            return Err(OwnerWireError::CorrelationMismatch("adoption_revision"));
        }

        let preview_digest =
            self.preview_manifest_digest
                .as_deref()
                .ok_or(OwnerWireError::CorrelationMismatch(
                    "adoption_source_digest",
                ))?;
        let approved_digest =
            self.approved_manifest_digest
                .as_deref()
                .ok_or(OwnerWireError::CorrelationMismatch(
                    "adoption_source_digest",
                ))?;
        validate_digest("adoption_preview_manifest", preview_digest)?;
        validate_digest("adoption_approved_manifest", approved_digest)?;
        if preview_digest != approved_digest {
            return Err(OwnerWireError::CorrelationMismatch(
                "adoption_source_digest",
            ));
        }

        // The caller has not supplied the requested source ID and its owner-advertised content
        // digest to this response validator. Do not treat an internally consistent, but
        // uncorrelated, digest pair as proof of source adoption.
        Err(OwnerWireError::CorrelationMismatch(
            "adoption_source_witness_required",
        ))
    }
}
