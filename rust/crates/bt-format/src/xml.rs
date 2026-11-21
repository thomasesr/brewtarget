// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/xml.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading BeerXML.
//!
//! Ported from `src/serialization/xml/BeerXml.cpp` and `XmlDocument.cpp`.
//!
//! BeerXML 1.0 is not valid XML, and the C++ says so at length.  The standard mandates an XML
//! declaration, which is normally optional; it omits to specify a single root element, which is
//! required of any XML document; and it writes booleans as `TRUE` and `FALSE` rather than the
//! lower case XML uses.  Brewtarget deals with the missing root by inserting `<BEER_XML>` after
//! the declaration and closing it at the end, which is what happens here too.
//!
//! The result is turned into the same shape a JSON document has, so that everything downstream --
//! the field mappings, the unit handling, the record graph -- is shared rather than written
//! twice.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XmlError {
    #[error("not valid XML: {0}")]
    Malformed(String),
    #[error("the file does not start with an XML declaration")]
    NoDeclaration,
}

/// The element the C++ wraps a BeerXML document in to give it a root.
pub const SYNTHETIC_ROOT: &str = "BEER_XML";

/// Give a BeerXML document the single root element it is missing.
///
/// The declaration stays on line one and the wrapper goes after it, so a reported line number is
/// one more than the line in the file the user has.
pub fn wrap(source: &str) -> Result<String, XmlError> {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("<?xml") {
        return Err(XmlError::NoDeclaration);
    }
    let (declaration, rest) = match trimmed.split_once("?>") {
        Some((declaration, rest)) => (format!("{declaration}?>"), rest),
        None => return Err(XmlError::NoDeclaration),
    };
    Ok(format!("{declaration}\n<{SYNTHETIC_ROOT}>{rest}\n</{SYNTHETIC_ROOT}>"))
}

/// Parse a wrapped BeerXML document into the same shape a JSON one has.
///
/// An element with children becomes an object; one with only text becomes that text.  An element
/// that appears more than once becomes an array, which is how a list of hops arrives.  Everything
/// is a string at this point: XML has no types, and the field definitions say what each one means.
pub fn to_value(source: &str) -> Result<Value, XmlError> {
    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);

    // A stack of part-built elements: the object being filled, and the text seen so far.
    let mut stack: Vec<(String, Map<String, Value>, String)> = vec![(String::new(), Map::new(), String::new())];

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = String::from_utf8_lossy(element.name().as_ref()).into_owned();
                stack.push((name, Map::new(), String::new()));
            }
            Ok(Event::Text(text)) => {
                let decoded = text.unescape().map_err(|error| XmlError::Malformed(error.to_string()))?;
                if let Some((_, _, buffer)) = stack.last_mut() {
                    buffer.push_str(decoded.as_ref());
                }
            }
            Ok(Event::End(_)) => {
                let Some((name, object, text)) = stack.pop() else {
                    return Err(XmlError::Malformed("an end tag with nothing open".to_owned()));
                };
                let value = if object.is_empty() { Value::String(text) } else { Value::Object(object) };
                let Some((_, parent, _)) = stack.last_mut() else {
                    return Err(XmlError::Malformed("an end tag outside the root".to_owned()));
                };
                add_child(parent, name, value);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(XmlError::Malformed(error.to_string())),
        }
    }

    let (_, root, _) = stack.pop().ok_or_else(|| XmlError::Malformed("nothing was read".to_owned()))?;
    Ok(Value::Object(root))
}

/// Add a child, turning the entry into an array once a name repeats.
///
/// This is how a list arrives in XML: `<HOPS>` holds any number of `<HOP>` elements, with nothing
/// in the document saying whether one or many is expected.
fn add_child(parent: &mut Map<String, Value>, name: String, value: Value) {
    match parent.get_mut(&name) {
        Some(Value::Array(existing)) => existing.push(value),
        Some(existing) => {
            let first = existing.take();
            *existing = Value::Array(vec![first, value]);
        }
        None => {
            parent.insert(name, value);
        }
    }
}

/// Read a BeerXML file into the shape the rest of the reader expects.
pub fn read(source: &str) -> Result<Value, XmlError> {
    to_value(&wrap(source)?)
}
