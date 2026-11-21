// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/jsonc.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! JSON that may contain comments.
//!
//! Strictly JSON has no comments, but `.beer` files are written by hand and use them heavily --
//! the example files in `testData/dotBeer/` are more comment than data in places -- and the C++
//! parses with Boost's `allow_comments` for exactly that reason.
//!
//! `serde_json` has no such option, so comments are blanked before parsing.  They are replaced
//! with spaces rather than removed so that every byte after them keeps its position, which means
//! a parse error still points at the right line and column of the file the user actually wrote.

/// Replace `//` and `/* */` comments with spaces, leaving everything else where it was.
///
/// Comment markers inside string literals are left alone: a hop called `Simcoe // 2024` is a
/// name, not a comment.
pub fn blank_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                // Copy the whole string literal, honouring backslash escapes.
                let start = index;
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
                let end = index.min(bytes.len());
                out.push_str(&source[start..end]);
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    out.push(' ');
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let start = index;
                index += 2;
                while index < bytes.len() && !(bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/')) {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
                // Keep the newlines so later line numbers do not shift.
                for byte in &bytes[start..index] {
                    out.push(if *byte == b'\n' { '\n' } else { ' ' });
                }
            }
            _ => {
                // Advance a whole character, since a multi-byte one must not be split.
                let remainder = &source[index..];
                let character = remainder.chars().next().expect("index is on a character boundary");
                out.push(character);
                index += character.len_utf8();
            }
        }
    }
    out
}

/// Parse JSON that may contain comments.
pub fn from_str<T: serde::de::DeserializeOwned>(source: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(&blank_comments(source))
}

/// Parse JSON that may contain comments into an untyped value.
pub fn value_from_str(source: &str) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_str(&blank_comments(source))
}

/// A JSON value parsed with duplicate object keys rejected.
///
/// RFC 8259 only says object names *should* be unique, and parsers differ over which value they
/// keep -- Boost.JSON and Python keep the last, others the first -- so dotBeer forbids them
/// outright.  A parser that silently keeps one would read such a file as if nothing were wrong,
/// which is exactly the case the format wants caught.
#[derive(Debug, Clone, PartialEq)]
pub struct Strict(pub serde_json::Value);

impl<'de> serde::Deserialize<'de> for Strict {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> serde::de::Visitor<'de> for StrictVisitor {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(serde_json::Number::from_f64(value).map_or(serde_json::Value::Null, Into::into))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(Strict(value)) = access.next_element()? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut object = serde_json::Map::new();
        while let Some(key) = access.next_key::<String>()? {
            let Strict(value) = access.next_value()?;
            if object.contains_key(&key) {
                return Err(serde::de::Error::custom(format!("duplicate key {key:?} in object")));
            }
            object.insert(key, value);
        }
        Ok(serde_json::Value::Object(object))
    }
}

/// Parse JSON that may contain comments, rejecting duplicate object keys.
pub fn strict_value_from_str(source: &str) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_str::<Strict>(&blank_comments(source)).map(|Strict(value)| value)
}
