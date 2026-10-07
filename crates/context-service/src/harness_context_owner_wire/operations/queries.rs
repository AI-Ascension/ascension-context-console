use serde::{Deserialize, Serialize};

use super::super::validation::{MAX_HARNESS_PAGE_SIZE, OwnerWireError, validate_identifier};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerItemsQueryV1 {
    pub draft_id: Option<String>,
    pub include_content: Option<bool>,
}

impl ContextOwnerItemsQueryV1 {
    pub(super) fn validate(&self) -> Result<(), OwnerWireError> {
        if let Some(draft_id) = &self.draft_id {
            validate_identifier("items_query_draft_id", draft_id)?;
        }
        Ok(())
    }

    pub(super) fn query(&self) -> Vec<super::types::HarnessQuery> {
        let mut query = Vec::with_capacity(2);
        if let Some(draft_id) = &self.draft_id {
            query.push(super::types::HarnessQuery {
                name: "draft_id".to_owned(),
                value: draft_id.clone(),
            });
        }
        if let Some(include_content) = self.include_content {
            query.push(super::types::HarnessQuery {
                name: "include_content".to_owned(),
                value: include_content.to_string(),
            });
        }
        query
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextOwnerRevisionQueryV1 {
    pub after_revision_id: Option<String>,
    pub limit: Option<u64>,
}

impl ContextOwnerRevisionQueryV1 {
    pub(super) fn validate(&self) -> Result<(), OwnerWireError> {
        if let Some(cursor) = &self.after_revision_id {
            validate_identifier("revision_cursor", cursor)?;
        }
        if self
            .limit
            .is_some_and(|limit| !(1..=MAX_HARNESS_PAGE_SIZE).contains(&limit))
        {
            return Err(OwnerWireError::OutOfBounds("revision_page_limit"));
        }
        Ok(())
    }

    pub(super) fn query(&self) -> Vec<super::types::HarnessQuery> {
        let mut query = Vec::with_capacity(2);
        if let Some(cursor) = &self.after_revision_id {
            query.push(super::types::HarnessQuery {
                name: "after_revision_id".to_owned(),
                value: cursor.clone(),
            });
        }
        if let Some(limit) = self.limit {
            query.push(super::types::HarnessQuery {
                name: "limit".to_owned(),
                value: limit.to_string(),
            });
        }
        query
    }
}
