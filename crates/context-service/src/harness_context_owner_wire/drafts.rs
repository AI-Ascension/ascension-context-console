#[path = "drafts/mutations.rs"]
mod mutations;
#[path = "drafts/records.rs"]
mod records;
#[path = "drafts/views.rs"]
mod views;

pub const CONTEXT_OWNER_DRAFT_SCHEMA_V1: &str = "ascension.harness.context-owner-draft.v1";
pub const CONTEXT_OWNER_REVISION_SCHEMA_V1: &str = "ascension.harness.context-owner-revision.v1";
pub const CONTEXT_OWNER_PREVIEW_SCHEMA_V1: &str = "ascension.harness.context-owner-preview.v1";
pub const CONTEXT_OWNER_MUTATION_RECEIPT_SCHEMA_V1: &str =
    "ascension.harness.context-owner-mutation-receipt.v1";
pub const CONTEXT_OWNER_ITEMS_SCHEMA_V1: &str = "ascension.harness.context-owner-items.v1";
pub const CONTEXT_OWNER_DRAFT_REQUEST_SCHEMA_V1: &str =
    "ascension.harness.context-owner-draft-request.v1";
pub const CONTEXT_OWNER_DRAFT_PATCH_SCHEMA_V1: &str =
    "ascension.harness.context-owner-draft-patch.v1";
pub const CONTEXT_OWNER_PREVIEW_REQUEST_SCHEMA_V1: &str =
    "ascension.harness.context-owner-preview-request.v1";
pub const CONTEXT_OWNER_MUTATION_LOOKUP_SCHEMA_V1: &str =
    "ascension.harness.context-owner-mutation-lookup.v1";

pub use mutations::{
    HarnessContextOwnerDraftCreateRequest, HarnessContextOwnerDraftOperation,
    HarnessContextOwnerDraftPatchRequest, HarnessContextOwnerMutationLookupRequest,
    HarnessContextOwnerMutationReceipt, HarnessContextOwnerMutationRequest,
    HarnessContextOwnerMutationResult, HarnessContextOwnerPreviewRequest,
};
pub use records::{ContextDraft, ContextItem, ContextItemRef, ContextNote};
pub use views::{
    HarnessContextOwnerDraftEnvelope, HarnessContextOwnerDraftListView,
    HarnessContextOwnerItemView, HarnessContextOwnerItemsView, HarnessContextOwnerPreviewEnvelope,
    HarnessContextOwnerRevisionEnvelope, HarnessContextOwnerRevisionPage,
};
