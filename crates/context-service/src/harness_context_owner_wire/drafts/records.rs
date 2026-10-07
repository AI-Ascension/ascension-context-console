use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::super::validation::{
    MAX_HARNESS_CONTEXT_BYTES, MAX_HARNESS_ITEMS, MAX_HARNESS_NOTES, OwnerWireError,
    validate_digest, validate_identifier, validate_schema,
};
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextItemRef {
    pub item_id: String,
    pub version: u64,
    pub sha256: String,
}

impl ContextItemRef {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_identifier("item_id", &self.item_id)?;
        validate_digest("item_sha256", &self.sha256)?;
        if self.version == 0 {
            return Err(OwnerWireError::InvalidValue("item_version"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextItem {
    pub reference: ContextItemRef,
    pub kind: String,
    pub bytes: Vec<u8>,
    pub protected: bool,
    pub expires_at: u64,
}

impl ContextItem {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        self.reference.validate()?;
        validate_identifier("item_kind", &self.kind)?;
        if self.bytes.is_empty() || self.bytes.len() > MAX_HARNESS_CONTEXT_BYTES {
            return Err(OwnerWireError::OutOfBounds("item_bytes"));
        }
        if self.expires_at == 0 {
            return Err(OwnerWireError::InvalidValue("item_expiry"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextNote {
    pub reference: ContextItemRef,
    pub attributed_to: String,
}

impl ContextNote {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        self.reference.validate()?;
        validate_identifier("note_author", &self.attributed_to)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextDraft {
    pub schema: String,
    pub draft_id: String,
    pub version: u64,
    pub base_revision_id: String,
    pub selected_items: Vec<ContextItemRef>,
    pub pinned_item_ids: Vec<String>,
    pub notes: Vec<ContextNote>,
    pub objective: Option<ContextItemRef>,
    pub author_ref: String,
}

impl ContextDraft {
    pub fn validate(&self) -> Result<(), OwnerWireError> {
        validate_schema(&self.schema, "ascension.context-control.draft.v1")?;
        validate_identifier("draft_id", &self.draft_id)?;
        validate_identifier("draft_base_revision_id", &self.base_revision_id)?;
        validate_identifier("draft_author_ref", &self.author_ref)?;
        if self.version == 0
            || self.selected_items.len() > MAX_HARNESS_ITEMS
            || self.pinned_item_ids.len() > MAX_HARNESS_ITEMS
            || self.notes.len() > MAX_HARNESS_NOTES
        {
            return Err(OwnerWireError::OutOfBounds("draft"));
        }
        let mut selected = BTreeSet::new();
        for item in &self.selected_items {
            item.validate()?;
            if !selected.insert((&item.item_id, item.version)) {
                return Err(OwnerWireError::InvalidValue("duplicate_selected_item"));
            }
        }
        let mut pinned = BTreeSet::new();
        for item_id in &self.pinned_item_ids {
            validate_identifier("pinned_item_id", item_id)?;
            if !pinned.insert(item_id) {
                return Err(OwnerWireError::InvalidValue("duplicate_pinned_item"));
            }
        }
        let mut notes = BTreeSet::new();
        for note in &self.notes {
            note.validate()?;
            if !notes.insert(&note.reference.item_id) {
                return Err(OwnerWireError::InvalidValue("duplicate_note"));
            }
        }
        if let Some(objective) = &self.objective {
            objective.validate()?;
        }
        Ok(())
    }
}
