// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/enums.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Translating a format's enum strings into the ones the database stores.
//!
//! Most of the time there is nothing to do: `.beer` and BeerJSON use the same strings the model
//! does, deliberately, so that one set of names serves everywhere.
//!
//! BeerXML does not.  It supports fewer hop forms and malt types than Brewtarget, so it declares
//! its own mappings, with its own capitalisation: `Pellet` where the database says `pellet`,
//! `Bittering` where it says `bittering`.  Writing BeerXML's spelling into the database would
//! produce a row the application cannot read back.
//!
//! The translation goes through the enum member, which is the thing both mappings agree on.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct EnumData {
    mappings: BTreeMap<String, Mapping>,
}

#[derive(Debug, Clone, Deserialize)]
struct Mapping {
    /// The enumeration this maps, e.g. `Hop::Form`.
    #[serde(rename = "enum")]
    enumeration: String,
    values: Vec<MappedValue>,
}

#[derive(Debug, Clone, Deserialize)]
struct MappedValue {
    member: String,
    string: String,
}

const ENUMS_JSON: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../schema/enums.json"));

fn enum_data() -> &'static EnumData {
    static DATA: OnceLock<EnumData> = OnceLock::new();
    DATA.get_or_init(|| serde_json::from_str(ENUMS_JSON).expect("schema/enums.json does not match bt_format::EnumData"))
}

/// Whether a mapping is one of a format's own rather than the model's.
///
/// The model declares its mappings on the class that owns the enum, so they are named
/// `Hop::formStringMapping`.  A format's own are declared at file scope with no owner at all.
fn is_a_formats_own(name: &str) -> bool {
    !name.contains("::")
}

/// Translate a value a format wrote into the one the database stores.
pub fn to_database_owned(mapping_name: &str, value: &str) -> Option<String> {
    let data = enum_data();
    let Some(mapping) = data.mappings.get(mapping_name) else {
        return Some(value.to_owned());
    };
    if !is_a_formats_own(mapping_name) {
        return Some(value.to_owned());
    }
    let member = &mapping.values.iter().find(|candidate| candidate.string == value)?.member;
    let model_mapping = data
        .mappings
        .iter()
        .find(|(name, candidate)| candidate.enumeration == mapping.enumeration && !is_a_formats_own(name))?
        .1;
    Some(model_mapping.values.iter().find(|candidate| candidate.member == *member)?.string.clone())
}

/// Translate the other way, for writing a file.
pub fn from_database(mapping_name: &str, value: &str) -> Option<String> {
    let data = enum_data();
    let Some(mapping) = data.mappings.get(mapping_name) else {
        return Some(value.to_owned());
    };
    if !is_a_formats_own(mapping_name) {
        return Some(value.to_owned());
    }
    let model_mapping = data
        .mappings
        .iter()
        .find(|(name, candidate)| candidate.enumeration == mapping.enumeration && !is_a_formats_own(name))?
        .1;
    let member = &model_mapping.values.iter().find(|candidate| candidate.string == value)?.member;
    Some(mapping.values.iter().find(|candidate| candidate.member == *member)?.string.clone())
}
