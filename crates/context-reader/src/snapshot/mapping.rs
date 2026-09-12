// SPDX-License-Identifier: MIT

use super::error::SnapshotError;
use super::types::Mapping;
use crate::json::{id_field, keys, strv, val};

const MAX_MAPPINGS: usize = 128;

pub(super) fn mappings(value: &crate::json::Value) -> Result<Vec<Mapping>, SnapshotError> {
    let values = value.array().ok_or(SnapshotError::Invalid("mapping"))?;
    if values.len() > MAX_MAPPINGS {
        return Err(SnapshotError::Invalid("mapping"));
    }
    let mut result = Vec::with_capacity(values.len());
    for item in values {
        let o = item.object().ok_or(SnapshotError::Invalid("mapping"))?;
        keys::<SnapshotError>(o, &["upstream_field", "transformation", "component_ids"])?;
        let transformation = strv::<SnapshotError>(o, "transformation")?;
        if !matches!(
            transformation.as_str(),
            "forwarded"
                | "rendered_as_text"
                | "mapped_to_schema"
                | "mapped_to_configuration"
                | "omitted"
                | "unsupported"
        ) {
            return Err(SnapshotError::Invalid("mapping.transformation"));
        }
        let values = val::<SnapshotError>(o, "component_ids")?
            .array()
            .ok_or(SnapshotError::Invalid("mapping.component_ids"))?;
        let mut component_ids = Vec::with_capacity(values.len());
        for value in values {
            let value = value
                .string()
                .ok_or(SnapshotError::Invalid("mapping.component_ids"))?;
            if !crate::json::identifier(value) {
                return Err(SnapshotError::Invalid("mapping.component_ids"));
            }
            if component_ids.iter().any(|old| old == value) {
                return Err(SnapshotError::Invalid("mapping.component_ids"));
            }
            component_ids.push(value.to_owned());
        }
        result.push(Mapping {
            upstream_field: id_field::<SnapshotError>(o, "upstream_field")?,
            transformation,
            component_ids,
        });
    }
    Ok(result)
}
