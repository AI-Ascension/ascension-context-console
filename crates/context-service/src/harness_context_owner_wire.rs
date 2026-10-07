//! Closed Console-side records for the versioned Harness context-owner protocol.
//!
//! These values describe a trusted local invocation and its exact Harness request/response.
//! `ContextOwnerInvocationV2` is a Console journal boundary, not an HTTP body to forward. The
//! `Harness*` request records serialize to the original Harness JSON shape; use `harness_body`
//! when producing a request body so Console identity and grant data never cross that boundary.

#[path = "harness_context_owner_wire/boundary.rs"]
mod boundary;
#[path = "harness_context_owner_wire/control.rs"]
mod control;
#[path = "harness_context_owner_wire/digest.rs"]
mod digest;
#[path = "harness_context_owner_wire/drafts.rs"]
mod drafts;
#[path = "harness_context_owner_wire/identity.rs"]
mod identity;
#[path = "harness_context_owner_wire/operations.rs"]
mod operations;
#[path = "harness_context_owner_wire/publication.rs"]
mod publication;
#[path = "harness_context_owner_wire/source.rs"]
mod source;
#[path = "harness_context_owner_wire/validation.rs"]
mod validation;

pub(crate) use validation::decode_bounded_json;

pub use boundary::{
    CONTEXT_OWNER_ASSOCIATION_VIEW_SCHEMA_V1, CONTEXT_OWNER_BINDING_SCHEMA_V1,
    CONTEXT_OWNER_EFFECTIVE_LIMITS_VIEW_SCHEMA_V1, ContextBindingContinuity, ContextBindingGrants,
    ContextBindingOperation, ContextBindingSource, ContextBindingState, ContextBoundary,
    ContextEffectiveLimits, ContextOwnerAssociationView, ContextOwnerBinding,
    ContextOwnerEffectiveLimitsView,
};
pub use control::{
    CONTEXT_OWNER_CONTROL_RECEIPT_SCHEMA_V2, ContextControlCommand, ContextControlCommandKind,
    ContextControlReceipt,
};
pub use digest::{DigestHex, HarnessDigestError};
pub use drafts::{
    CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1, CONTEXT_OWNER_DRAFT_REQUEST_SCHEMA_V1,
    CONTEXT_OWNER_DRAFT_SCHEMA_V1, CONTEXT_OWNER_ITEMS_SCHEMA_V1,
    CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1, CONTEXT_OWNER_MUTATION_RECEIPT_SCHEMA_V1,
    CONTEXT_OWNER_PREVIEW_REQUEST_SCHEMA_V1, CONTEXT_OWNER_PREVIEW_SCHEMA_V1,
    CONTEXT_OWNER_REVISION_SCHEMA_V1, ContextDraft, ContextItem, ContextItemRef, ContextNote,
    HarnessContextOwnerDraftCreateRequest, HarnessContextOwnerDraftEnvelope,
    HarnessContextOwnerDraftListView, HarnessContextOwnerDraftOperation,
    HarnessContextOwnerDraftPatchRequest, HarnessContextOwnerItemsView,
    HarnessContextOwnerMutationLookupRequest, HarnessContextOwnerMutationReceipt,
    HarnessContextOwnerMutationRequest, HarnessContextOwnerMutationResult,
    HarnessContextOwnerPreviewEnvelope, HarnessContextOwnerPreviewRequest,
    HarnessContextOwnerRevisionEnvelope, HarnessContextOwnerRevisionPage,
};
pub use identity::{
    CONSOLE_CONTEXT_OWNER_INVOCATION_SCHEMA_V2, ConsoleOwnerIdentityV2, ConsoleOwnerScopeV2,
    HarnessActorIdentityV2, OwnerIdentityCorrelationV2,
};
pub use operations::{
    ContextOwnerEndpointV1, ContextOwnerInvocationV2, ContextOwnerItemsQueryV1,
    ContextOwnerOperationV2, ContextOwnerRevisionQueryV1, HarnessHttpMethod, HarnessQuery,
    HarnessResponseV1,
};
pub use publication::{
    CONTEXT_OWNER_PUBLICATION_LOOKUP_SCHEMA_V1, CONTEXT_OWNER_PUBLICATION_RECEIPT_SCHEMA_V1,
    CONTEXT_OWNER_PUBLICATION_REQUEST_SCHEMA_V1, CONTEXT_OWNER_PUBLISHED_SOURCES_SCHEMA_V1,
    HarnessContextOwnerDraftPublicationLookupRequest, HarnessContextOwnerDraftPublicationReceipt,
    HarnessContextOwnerDraftPublicationRequest, HarnessContextOwnerPublishedSourcesView,
};
pub use source::{
    CONTEXT_OWNER_SOURCE_STATUS_SCHEMA_V1, CONTEXT_SOURCE_ADOPTION_SCHEMA_V1,
    CONTEXT_SOURCE_UPLOAD_SCHEMA_V1, ContextOwnerSourceStatus, ContextSourceAdoptionRequest,
    ContextSourceDocument, ContextSourcePublication, ContextSourceUpload,
};
pub use validation::{
    MAX_HARNESS_CONTEXT_BYTES, MAX_HARNESS_DRAFT_OPERATIONS, MAX_HARNESS_ITEMS,
    MAX_HARNESS_JSON_BODY_BYTES, MAX_HARNESS_JSON_DEPTH, MAX_HARNESS_NOTE_BYTES, MAX_HARNESS_NOTES,
    MAX_HARNESS_OBJECTIVE_BYTES, MAX_HARNESS_PAGE_SIZE, MAX_HARNESS_PUBLICATIONS, OwnerWireError,
};

#[cfg(test)]
#[path = "harness_context_owner_wire/tests.rs"]
mod tests;
