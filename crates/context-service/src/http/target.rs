// SPDX-License-Identifier: MIT

use super::error::ApiError;
use crate::read_api::valid_id;

pub(crate) fn split_target(target: &str) -> (&str, Vec<(String, String)>) {
    let Some((path, query)) = target.split_once('?') else {
        return (target, Vec::new());
    };
    let values = query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (key.to_owned(), value.to_owned())
        })
        .collect();
    (path, values)
}

pub(crate) fn query_limit(query: &[(String, String)]) -> Result<usize, ApiError> {
    let value = query
        .iter()
        .find(|(key, _)| key == "limit")
        .map(|(_, value)| value.as_str())
        .unwrap_or("50");
    let limit = value.parse::<usize>().map_err(|_| ApiError::BadRequest)?;
    if limit == 0 || limit > 200 {
        return Err(ApiError::TooLarge);
    }
    Ok(limit)
}

pub(crate) fn query_value<'a>(
    query: &'a [(String, String)],
    key: &str,
) -> Result<&'a str, ApiError> {
    query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.is_empty() && valid_id(value))
        .ok_or(ApiError::BadRequest)
}
