use std::collections::HashSet;
use std::fmt;

use serde::Serialize;
use serde::de::{DeserializeSeed, Deserializer, Error as DeError, MapAccess, SeqAccess, Visitor};

pub const MAX_HARNESS_JSON_BODY_BYTES: usize = 1024 * 1024;
pub const MAX_HARNESS_JSON_DEPTH: usize = 32;
pub const MAX_HARNESS_ITEMS: usize = 64;
pub const MAX_HARNESS_NOTES: usize = 16;
pub const MAX_HARNESS_CONTEXT_BYTES: usize = 128 * 1024;
pub const MAX_HARNESS_NOTE_BYTES: usize = 4 * 1024;
pub const MAX_HARNESS_OBJECTIVE_BYTES: usize = 512;
pub const MAX_HARNESS_DRAFT_OPERATIONS: usize = 32;
pub const MAX_HARNESS_PAGE_SIZE: u64 = 50;
pub const MAX_HARNESS_PUBLICATIONS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OwnerWireError {
    UnsupportedSchema(&'static str),
    InvalidIdentifier(&'static str),
    InvalidDigest(&'static str),
    InvalidValue(&'static str),
    OutOfBounds(&'static str),
    CorrelationMismatch(&'static str),
    JsonEncoding,
    JsonDecoding,
}

impl fmt::Display for OwnerWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema(field) => write!(f, "unsupported schema: {field}"),
            Self::InvalidIdentifier(field) => write!(f, "invalid identifier: {field}"),
            Self::InvalidDigest(field) => write!(f, "invalid digest: {field}"),
            Self::InvalidValue(field) => write!(f, "invalid value: {field}"),
            Self::OutOfBounds(field) => write!(f, "field outside bound: {field}"),
            Self::CorrelationMismatch(field) => write!(f, "identity mismatch: {field}"),
            Self::JsonEncoding => f.write_str("owner JSON could not be encoded"),
            Self::JsonDecoding => f.write_str("owner JSON could not be decoded"),
        }
    }
}

impl std::error::Error for OwnerWireError {}

pub(crate) fn validate_schema(actual: &str, expected: &'static str) -> Result<(), OwnerWireError> {
    if actual == expected {
        Ok(())
    } else {
        Err(OwnerWireError::UnsupportedSchema(expected))
    }
}

pub(crate) fn validate_identifier(field: &'static str, value: &str) -> Result<(), OwnerWireError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(byte))
    {
        return Err(OwnerWireError::InvalidIdentifier(field));
    }
    Ok(())
}

/// Bounds an authenticated identity value without treating URLs or opaque provider IDs as path
/// identifiers. The value is retained exactly for local correlation and is never used in a URL.
pub(crate) fn validate_correlation_text(
    field: &'static str,
    value: &str,
) -> Result<(), OwnerWireError> {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(OwnerWireError::InvalidIdentifier(field));
    }
    Ok(())
}

pub(crate) fn validate_digest(field: &'static str, value: &str) -> Result<(), OwnerWireError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OwnerWireError::InvalidDigest(field));
    }
    Ok(())
}

pub(crate) fn bounded_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, OwnerWireError> {
    let bytes = serde_json::to_vec(value).map_err(|_| OwnerWireError::JsonEncoding)?;
    validate_json_shape(&bytes).map_err(|error| match error {
        OwnerWireError::JsonDecoding => OwnerWireError::JsonEncoding,
        other => other,
    })?;
    Ok(bytes)
}

pub(crate) fn decode_bounded_json<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, OwnerWireError> {
    validate_json_shape(bytes)?;
    serde_json::from_slice(bytes).map_err(|_| OwnerWireError::JsonDecoding)
}

/// Scans the JSON token stream without materializing a generic value tree. Object keys are
/// decoded as strings and checked before each typed decode, so duplicate struct fields and
/// duplicate keys inside maps cannot be normalized by an intermediate map representation.
pub(super) fn validate_json_shape(bytes: &[u8]) -> Result<(), OwnerWireError> {
    if bytes.len() > MAX_HARNESS_JSON_BODY_BYTES {
        return Err(OwnerWireError::OutOfBounds("json_body"));
    }

    // Every JSON value consumes at least one input byte; the extra slot covers the root.
    // The prechecked body ceiling therefore bounds both visits and duplicate-key tracking.
    let mut budget = JsonShapeBudget {
        nodes: 0,
        max_nodes: bytes.len().saturating_add(1),
        failure: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let scanned = JsonShapeSeed {
        depth: 1,
        budget: &mut budget,
    }
    .deserialize(&mut deserializer);
    if scanned.is_err() {
        return Err(budget
            .failure
            .take()
            .unwrap_or(OwnerWireError::JsonDecoding));
    }
    deserializer.end().map_err(|_| OwnerWireError::JsonDecoding)
}

#[derive(Default)]
struct JsonShapeBudget {
    nodes: usize,
    max_nodes: usize,
    failure: Option<OwnerWireError>,
}

impl JsonShapeBudget {
    fn enter<E: DeError>(&mut self, depth: usize) -> Result<(), E> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| E::custom("JSON node budget overflow"))?;
        if self.nodes > self.max_nodes || depth > MAX_HARNESS_JSON_DEPTH {
            self.failure = Some(OwnerWireError::OutOfBounds("json_shape"));
            return Err(E::custom("JSON shape exceeds its bound"));
        }
        Ok(())
    }

    fn duplicate_key<E: DeError>(&mut self) -> E {
        self.failure = Some(OwnerWireError::JsonDecoding);
        E::custom("duplicate JSON object key")
    }
}

struct JsonShapeSeed<'a> {
    depth: usize,
    budget: &'a mut JsonShapeBudget,
}

impl<'de> DeserializeSeed<'de> for JsonShapeSeed<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.budget.enter::<D::Error>(self.depth)?;
        deserializer.deserialize_any(JsonShapeVisitor {
            depth: self.depth,
            budget: self.budget,
        })
    }
}

struct JsonShapeVisitor<'a> {
    depth: usize,
    budget: &'a mut JsonShapeBudget,
}

impl<'de> Visitor<'de> for JsonShapeVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON value with unique object keys")
    }

    fn visit_bool<E: DeError>(self, _: bool) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_i64<E: DeError>(self, _: i64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_u64<E: DeError>(self, _: u64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_f64<E: DeError>(self, _: f64) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_str<E: DeError>(self, _: &str) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_string<E: DeError>(self, _: String) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        JsonShapeSeed {
            depth: self.depth + 1,
            budget: self.budget,
        }
        .deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        while sequence
            .next_element_seed(JsonShapeSeed {
                depth: self.depth + 1,
                budget: &mut *self.budget,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key) {
                return Err(self.budget.duplicate_key::<A::Error>());
            }
            map.next_value_seed(JsonShapeSeed {
                depth: self.depth + 1,
                budget: &mut *self.budget,
            })?;
        }
        Ok(())
    }
}
