// SPDX-License-Identifier: MIT

use std::fmt;

const MAX_DEPTH: usize = 32;
const MAX_ITEMS: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub(crate) fn object(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Object(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    pub(crate) fn string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn number(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn boolean(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Unexpected,
    InvalidUtf8,
    InvalidEscape,
    InvalidNumber,
    DuplicateKey,
    TooDeep,
    TooManyItems,
    TrailingBytes,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Unexpected => "unexpected JSON input",
            Self::InvalidUtf8 => "JSON string is not UTF-8",
            Self::InvalidEscape => "invalid JSON string escape",
            Self::InvalidNumber => "JSON number is not a bounded unsigned integer",
            Self::DuplicateKey => "JSON object contains a duplicate key",
            Self::TooDeep => "JSON nesting exceeds its bound",
            Self::TooManyItems => "JSON object or array exceeds its item bound",
            Self::TrailingBytes => "JSON has trailing non-whitespace bytes",
        };
        formatter.write_str(message)
    }
}

pub(crate) fn parse(input: &[u8]) -> Result<Value, Error> {
    let mut parser = Parser {
        input,
        index: 0,
        depth: 0,
        items: 0,
    };
    let value = parser.value()?;
    parser.whitespace();
    if parser.index == input.len() {
        Ok(value)
    } else {
        Err(Error::TrailingBytes)
    }
}

struct Parser<'a> {
    input: &'a [u8],
    index: usize,
    depth: usize,
    items: usize,
}

impl Parser<'_> {
    fn value(&mut self) -> Result<Value, Error> {
        self.whitespace();
        let byte = *self.input.get(self.index).ok_or(Error::Unexpected)?;
        match byte {
            b'n' => self.literal(b"null", Value::Null),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'"' => self.string().map(Value::String),
            b'[' => self.array(),
            b'{' => self.object(),
            b'0'..=b'9' => self.number().map(Value::Number),
            _ => Err(Error::Unexpected),
        }
    }

    fn literal(&mut self, expected: &[u8], value: Value) -> Result<Value, Error> {
        let end = self
            .index
            .checked_add(expected.len())
            .ok_or(Error::Unexpected)?;
        if self.input.get(self.index..end) == Some(expected) {
            self.index = end;
            Ok(value)
        } else {
            Err(Error::Unexpected)
        }
    }

    fn string(&mut self) -> Result<String, Error> {
        if self.input.get(self.index) != Some(&b'"') {
            return Err(Error::Unexpected);
        }
        self.index += 1;
        let mut bytes = Vec::new();
        while let Some(&byte) = self.input.get(self.index) {
            self.index += 1;
            match byte {
                b'"' => return String::from_utf8(bytes).map_err(|_| Error::InvalidUtf8),
                b'\\' => self.escape(&mut bytes)?,
                0..=0x1f => return Err(Error::Unexpected),
                _ => bytes.push(byte),
            }
        }
        Err(Error::Unexpected)
    }

    fn escape(&mut self, bytes: &mut Vec<u8>) -> Result<(), Error> {
        let escaped = *self.input.get(self.index).ok_or(Error::InvalidEscape)?;
        self.index += 1;
        match escaped {
            b'"' | b'\\' | b'/' => bytes.push(escaped),
            b'b' => bytes.push(0x08),
            b'f' => bytes.push(0x0c),
            b'n' => bytes.push(b'\n'),
            b'r' => bytes.push(b'\r'),
            b't' => bytes.push(b'\t'),
            b'u' => {
                let code = self.hex_quad()?;
                let character = char::from_u32(code).ok_or(Error::InvalidEscape)?;
                let mut encoded = [0_u8; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
            }
            _ => return Err(Error::InvalidEscape),
        }
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u32, Error> {
        let mut value = 0_u32;
        for _ in 0..4 {
            let digit = *self.input.get(self.index).ok_or(Error::InvalidEscape)?;
            self.index += 1;
            value = value
                .checked_mul(16)
                .and_then(|value| value.checked_add(hex_value(digit)))
                .ok_or(Error::InvalidEscape)?;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<u64, Error> {
        let start = self.index;
        while matches!(self.input.get(self.index), Some(b'0'..=b'9')) {
            self.index += 1;
        }
        if self.index == start {
            return Err(Error::InvalidNumber);
        }
        if matches!(
            self.input.get(self.index),
            Some(b'.' | b'e' | b'E' | b'-' | b'+')
        ) {
            return Err(Error::InvalidNumber);
        }
        std::str::from_utf8(&self.input[start..self.index])
            .map_err(|_| Error::InvalidNumber)?
            .parse()
            .map_err(|_| Error::InvalidNumber)
    }

    fn array(&mut self) -> Result<Value, Error> {
        self.enter()?;
        self.index += 1;
        let mut values = Vec::new();
        self.whitespace();
        if self.take(b']') {
            self.leave();
            return Ok(Value::Array(values));
        }
        loop {
            self.whitespace();
            self.item()?;
            values.push(self.value()?);
            self.whitespace();
            if self.take(b']') {
                self.leave();
                return Ok(Value::Array(values));
            }
            if !self.take(b',') {
                return Err(Error::Unexpected);
            }
        }
    }

    fn object(&mut self) -> Result<Value, Error> {
        self.enter()?;
        self.index += 1;
        let mut values = Vec::new();
        self.whitespace();
        if self.take(b'}') {
            self.leave();
            return Ok(Value::Object(values));
        }
        loop {
            self.whitespace();
            self.item()?;
            let key = self.string()?;
            if values.iter().any(|(existing, _)| existing == &key) {
                return Err(Error::DuplicateKey);
            }
            self.whitespace();
            if !self.take(b':') {
                return Err(Error::Unexpected);
            }
            let value = self.value()?;
            values.push((key, value));
            self.whitespace();
            if self.take(b'}') {
                self.leave();
                return Ok(Value::Object(values));
            }
            if !self.take(b',') {
                return Err(Error::Unexpected);
            }
        }
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth = self.depth.checked_add(1).ok_or(Error::TooDeep)?;
        if self.depth > MAX_DEPTH {
            Err(Error::TooDeep)
        } else {
            Ok(())
        }
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn item(&mut self) -> Result<(), Error> {
        self.items = self.items.checked_add(1).ok_or(Error::TooManyItems)?;
        if self.items > MAX_ITEMS {
            Err(Error::TooManyItems)
        } else {
            Ok(())
        }
    }

    fn take(&mut self, expected: u8) -> bool {
        if self.input.get(self.index) == Some(&expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn whitespace(&mut self) {
        while matches!(
            self.input.get(self.index),
            Some(b' ' | b'\n' | b'\r' | b'\t')
        ) {
            self.index += 1;
        }
    }
}

fn hex_value(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => u32::from(byte - b'0'),
        b'a'..=b'f' => u32::from(byte - b'a' + 10),
        b'A'..=b'F' => u32::from(byte - b'A' + 10),
        _ => u32::MAX,
    }
}
