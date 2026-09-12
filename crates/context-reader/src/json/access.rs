// SPDX-License-Identifier: MIT

use super::value::Value;

pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub(crate) type Object = [(String, Value)];

pub(crate) trait AccessError {
    fn invalid(field: &'static str) -> Self;
}

pub(crate) fn keys<E: AccessError>(o: &Object, names: &[&'static str]) -> Result<(), E> {
    if names.iter().any(|name| field(o, name).is_none())
        || o.iter().any(|(key, _)| !names.contains(&key.as_str()))
    {
        return Err(E::invalid("unknown or missing field"));
    }
    Ok(())
}

pub(crate) fn field<'a>(o: &'a Object, name: &str) -> Option<&'a Value> {
    o.iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

pub(crate) fn val<'a, E: AccessError>(o: &'a Object, name: &'static str) -> Result<&'a Value, E> {
    field(o, name).ok_or(E::invalid(name))
}

pub(crate) fn obj<'a, E: AccessError>(o: &'a Object, name: &'static str) -> Result<&'a Object, E> {
    val(o, name)?.object().ok_or(E::invalid(name))
}

pub(crate) fn strv<E: AccessError>(o: &Object, name: &'static str) -> Result<String, E> {
    val(o, name)?
        .string()
        .map(str::to_owned)
        .ok_or(E::invalid(name))
}

pub(crate) fn bounded<E: AccessError>(
    o: &Object,
    name: &'static str,
    max: usize,
) -> Result<String, E> {
    let value = strv(o, name)?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(E::invalid(name));
    }
    Ok(value)
}

pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && "._:-".contains(character))
        })
}

pub(crate) fn id_field<E: AccessError>(o: &Object, name: &'static str) -> Result<String, E> {
    let value = bounded(o, name, 128)?;
    if !identifier(&value) {
        return Err(E::invalid(name));
    }
    Ok(value)
}

pub(crate) fn optional_id<E: AccessError>(
    o: &Object,
    name: &'static str,
) -> Result<Option<String>, E> {
    match field(o, name) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if identifier(value) => Ok(Some(value.clone())),
        Some(_) => Err(E::invalid(name)),
        None => Ok(None),
    }
}

pub(crate) fn number<E: AccessError>(o: &Object, name: &'static str) -> Result<u64, E> {
    let value = val(o, name)?.number().ok_or(E::invalid(name))?;
    if value > MAX_SAFE_INTEGER {
        return Err(E::invalid(name));
    }
    Ok(value)
}

pub(crate) fn optional_number<E: AccessError>(
    o: &Object,
    name: &'static str,
) -> Result<Option<u64>, E> {
    let value = val(o, name)?;
    if value.is_null() {
        Ok(None)
    } else {
        let value = value.number().ok_or(E::invalid(name))?;
        if value > MAX_SAFE_INTEGER {
            return Err(E::invalid(name));
        }
        Ok(Some(value))
    }
}

pub(crate) fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return false;
    }
    if ![0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
        .into_iter()
        .all(|range| bytes[range].iter().all(u8::is_ascii_digit))
    {
        return false;
    }
    let rest = &bytes[19..];
    let zone = if rest.first() == Some(&b'.') {
        let Some(zone_start) = rest.iter().position(|byte| *byte == b'Z' || *byte == b'z') else {
            return false;
        };
        if zone_start == 1 || !rest[1..zone_start].iter().all(u8::is_ascii_digit) {
            return false;
        }
        &rest[zone_start..]
    } else {
        rest
    };
    if matches!(zone, [b'Z'] | [b'z']) {
        return true;
    }
    zone.len() == 6
        && matches!(zone[0], b'+' | b'-')
        && zone[3] == b':'
        && zone[1..3].iter().all(u8::is_ascii_digit)
        && zone[4..6].iter().all(u8::is_ascii_digit)
}
