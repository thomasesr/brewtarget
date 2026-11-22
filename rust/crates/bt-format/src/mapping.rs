// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/mapping.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Turning a file's records into database rows.
//!
//! Ported from the reading half of `src/serialization/json/JsonRecord.cpp`.
//!
//! A format names a model property; the database names a column; `schema/formats.json` and
//! `schema/v23.json` between them know how to get from one to the other.  Nothing here is
//! hand-written per entity: the mapping is walked, and the result is a column-keyed object that
//! deserialises straight into the generated row structs.
//!
//! Values do not arrive in the units we store.  A measurement carries its own unit and is
//! converted to canonical; a percentage or a pH is in a unit the field always uses.  That is why
//! this depends on `bt-measure` rather than just copying numbers across.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::data::{Field, Format, Record, UnitMapping, format_data};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MappingError {
    #[error("{0} is not a format this build knows about")]
    UnknownFormat(String),
    #[error("{format} has no record called {record}")]
    UnknownRecord { format: String, record: String },
    #[error("{cpp_type} is not stored in any table")]
    NoTableFor { cpp_type: String },
    #[error("{path}: {message}")]
    BadValue { path: String, message: String },
}

/// One record read out of a file, ready to be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct MappedRecord {
    /// The table this belongs in, e.g. `hop`.
    pub table: &'static str,
    /// The columns, keyed by column name, ready to deserialise into a row struct.
    pub columns: Map<String, Value>,
    /// The file-local identifier, where the record has one.
    ///
    /// Only the field actually called `local_id` is the record's own: every other `LocalId`
    /// field is a reference to a different record, and a recipe has seven of those and no
    /// identity of its own.
    pub local_id: Option<String>,
    /// References to other records, by the field that holds them -- `style_id`, `hop_id` and so
    /// on.  These become foreign keys once the records they name have been stored.
    pub references: BTreeMap<String, String>,
    /// The folder this record says it lives in, which has to become a folder row of its own.
    pub folder_path: Option<String>,
    /// Values the record carried that no column on this table holds, keyed by the property path
    /// that named them, joined with `/`.
    ///
    /// Two sorts end up here.  Some are calculated -- a recipe's ABV is written out so that other
    /// programs can read it, and recalculated rather than read back.  The rest describe something
    /// the record does not itself hold: BeerXML has no `Boil` record, so a recipe's `BOIL_TIME`
    /// arrives as `Recipe::boil/Boil::boilTime_mins` and a boil has to be made to put it in.  See
    /// `crate::legacy`.
    pub legacy: BTreeMap<String, Value>,
}

impl MappedRecord {
    /// Deserialise into one of the generated row structs.
    pub fn into_row<T: serde::de::DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        serde_json::from_value(Value::Object(self.columns))
    }
}

/// Follow a `/`-separated path into a record.
fn value_at<'a>(record: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = record;
    for part in path.split('/') {
        current = current.get(part)?;
    }
    Some(current)
}

/// The unit a measurement names, mapped onto ours.
fn unit_named(format: &Format, mapping_name: &str, unit_name: &str) -> Option<bt_measure::Unit> {
    match format.unit_mappings.get(mapping_name)? {
        UnitMapping::Units { units } => units.get(unit_name).and_then(|name| bt_measure::Unit::by_name(name)),
        UnitMapping::SingleUnit { .. } => None,
        // A field that accepts a mass or a volume: try each table until one recognises the unit.
        UnitMapping::AnyOf { mappings } => mappings.iter().find_map(|mapping| unit_named(format, mapping, unit_name)),
    }
}

/// The unit a single-unit field is always in.
///
/// The mapping names it -- "%" for a percentage, "pH" for acidity -- which is what gets written
/// back out.  Those are not units we convert through, so the value is stored as given.
fn single_unit_is_dimensionless(format: &Format, mapping_name: &str) -> bool {
    matches!(format.unit_mappings.get(mapping_name), Some(UnitMapping::SingleUnit { .. }))
}

/// Read a boolean, accepting the capitalised spellings BeerXML uses.
fn as_bool(raw: &Value) -> Option<bool> {
    raw.as_bool().or_else(|| match raw.as_str()?.trim() {
        "true" | "TRUE" | "True" | "1" => Some(true),
        "false" | "FALSE" | "False" | "0" => Some(false),
        _ => None,
    })
}

/// Read a whole number, from a JSON number or from text.
fn as_i64(raw: &Value) -> Option<i64> {
    raw.as_i64().or_else(|| raw.as_str()?.trim().parse().ok()).or_else(|| as_f64(raw).map(|value| value as i64))
}

/// Read a number, from a JSON number or from text.
fn as_f64(raw: &Value) -> Option<f64> {
    raw.as_f64().or_else(|| raw.as_str()?.trim().parse().ok())
}

/// Read a `{"unit": …, "value": …}` pair, resolving the unit through the field's table.
fn measurement(format: &Format, field: &Field, raw: &Value) -> Result<(f64, bt_measure::Unit), MappingError> {
    let bad = |message: String| MappingError::BadValue { path: field.path.clone(), message };
    let quantity = raw.get("value").and_then(Value::as_f64).ok_or_else(|| bad("expected a value".to_owned()))?;
    let unit_name = raw.get("unit").and_then(Value::as_str).ok_or_else(|| bad("expected a unit".to_owned()))?;
    let mapping = field.decoder.as_deref().ok_or_else(|| bad("no unit table for this measurement".to_owned()))?;
    let unit = unit_named(format, mapping, unit_name)
        .ok_or_else(|| bad(format!("{unit_name:?} is not a unit {mapping} knows")))?;
    Ok((quantity, unit))
}

/// Read one field out of a record.
///
/// Returns `None` when the field is simply absent, which is the common case: these formats make
/// almost everything optional and a reader must not invent values for what is missing.
fn read_field(format: &Format, record: &Value, field: &Field) -> Result<Option<Value>, MappingError> {
    let Some(raw) = value_at(record, &field.path) else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let bad = |message: &str| MappingError::BadValue { path: field.path.clone(), message: message.to_owned() };

    let value = match field.field_type.as_str() {
        "String" | "LocalId" | "FolderPath" => {
            Value::String(raw.as_str().ok_or_else(|| bad("expected a string"))?.to_owned())
        }

        // A unit column holds the unit's own name.  BeerXML has no such field in the standard --
        // amounts there are implicitly in kilograms or litres -- so Brewtarget writes one of its
        // own to keep the unit the user chose.
        "Unit" => Value::String(raw.as_str().ok_or_else(|| bad("expected a unit name"))?.to_owned()),

        // The database keeps dates as dates.  A file may give a full timestamp -- a brew started
        // at half past two in the afternoon -- and storing that into a DATE column keeps the day
        // and drops the rest, which is what the C++ does when it puts a QDateTime into a QDate.
        // The date taken is the one as written, so a local time stays on its local day.
        "Date" | "DateTimeOffset" | "DateOrDateTimeLocal" => {
            let text = raw.as_str().ok_or_else(|| bad("expected a date"))?;
            Value::String(text.split('T').next().unwrap_or(text).to_owned())
        }
        // XML carries no types, so every value arrives as text and has to be coerced.  BeerXML
        // writes booleans as TRUE and FALSE in capitals, which the standard specifies and XML
        // does not allow, so both spellings are accepted.
        "Bool" => Value::Bool(as_bool(raw).ok_or_else(|| bad("expected true or false"))?),
        "Int" | "UInt" => Value::from(as_i64(raw).ok_or_else(|| bad("expected a whole number"))?),
        "Double" => Value::from(as_f64(raw).ok_or_else(|| bad("expected a number"))?),

        // An enum arrives as whatever string the format uses.  For `.beer` and BeerJSON that is
        // the string the database stores too, deliberately; BeerXML uses its own, so the value
        // is translated through the enum member both mappings agree on.
        "Enum" => {
            let text = raw.as_str().ok_or_else(|| bad("expected a string"))?;
            let translated = match field.decoder.as_deref() {
                Some(mapping) => crate::enums::to_database_owned(mapping, text)
                    .ok_or_else(|| bad(&format!("{text:?} is not a value {mapping} knows")))?,
                None => text.to_owned(),
            };
            Value::String(translated)
        }

        // A measurement is a value and a unit together.  Most columns hold one number, so the
        // value is converted to the canonical unit -- five US gallons becomes 18.927 litres,
        // because that is what the column means.  An ingredient amount is the exception, and is
        // handled by the caller, which has two columns to fill.
        "MeasurementWithUnits" | "OneOfMeasurementsWithUnits" => {
            let (quantity, unit) = measurement(format, field, raw)?;
            Value::from(unit.to_canonical(quantity).quantity)
        }

        // A single-unit field is always in the one unit, so only the number is interesting.
        "SingleUnitValue" => {
            let quantity =
                raw.get("value").and_then(as_f64).or_else(|| as_f64(raw)).ok_or_else(|| bad("expected a value"))?;
            if let Some(mapping) = field.decoder.as_deref()
                && !single_unit_is_dimensionless(format, mapping)
                && let Some(unit_name) = raw.get("unit").and_then(Value::as_str)
                && let Some(unit) = unit_named(format, mapping, unit_name)
            {
                Value::from(unit.to_canonical(quantity).quantity)
            } else {
                Value::from(quantity)
            }
        }

        // Structural rather than a value: handled by the caller, which has to create rows of its
        // own for whatever is nested.
        "Record" | "ListOfRecords" | "RequiredConstant" => return Ok(None),

        other => return Err(bad(&format!("field type {other} is not handled"))),
    };
    Ok(Some(value))
}

/// Read one record of a named collection into its columns.
pub fn map_record(format_name: &str, record_name: &str, record: &Value) -> Result<MappedRecord, MappingError> {
    let format =
        format_data().format(format_name).ok_or_else(|| MappingError::UnknownFormat(format_name.to_owned()))?;
    let definition = format.record_named(record_name).ok_or_else(|| MappingError::UnknownRecord {
        format: format_name.to_owned(),
        record: record_name.to_owned(),
    })?;
    map_record_with(format, definition, record)
}

/// Read one record against a definition already in hand.
pub fn map_record_with(format: &Format, definition: &Record, record: &Value) -> Result<MappedRecord, MappingError> {
    let table = bt_model::tables::table_for_cpp_type(&definition.cpp_type)
        .ok_or_else(|| MappingError::NoTableFor { cpp_type: definition.cpp_type.clone() })?;

    let mut columns = Map::new();
    let mut local_id = None;
    let mut folder_path = None;
    let mut references = BTreeMap::new();
    let mut legacy = BTreeMap::new();

    for field in &definition.fields {
        let Some(value) = read_field(format, record, field)? else {
            continue;
        };
        // A field naming a chain of properties is describing a record other than this one, so
        // there is nothing on this table to put it in.  It is kept under the whole chain, which
        // is what says where it belongs.
        if let Some(path) = &field.property_path {
            legacy.insert(path.join("/"), value);
            continue;
        }
        match field.field_type.as_str() {
            // Neither of these is a column: a local ID exists only within the file, and a folder
            // path has to become a folder row before anything can point at it.
            "LocalId" if field.path == "local_id" => local_id = value.as_str().map(str::to_owned),
            "LocalId" => {
                if let Some(reference) = value.as_str() {
                    references.insert(field.path.clone(), reference.to_owned());
                }
            }
            "FolderPath" => folder_path = value.as_str().map(str::to_owned),
            _ => {
                let Some(property) = field.property.as_deref() else { continue };
                // An ingredient amount is one field in the file but two columns in the database:
                // how much, and in what unit.  Both are stored as given rather than converted,
                // because the unit the user chose is part of what the recipe says -- 15 g of
                // hops reads differently from 0.015 kg.
                if property.ends_with("::amount")
                    && let (Some(quantity_column), Some(unit_column)) = (
                        bt_model::tables::column_for_property(table, "IngredientAmount::quantity"),
                        bt_model::tables::column_for_property(table, "IngredientAmount::unit"),
                    )
                    && let Some(raw) = value_at(record, &field.path)
                    && let Ok((quantity, unit)) = measurement(format, field, raw)
                {
                    columns.insert(quantity_column.to_owned(), Value::from(quantity));
                    columns.insert(unit_column.to_owned(), Value::String(unit.name().to_owned()));
                    continue;
                }
                match bt_model::tables::column_for_property(table, property) {
                    Some(column) => {
                        columns.insert(column.to_owned(), value);
                    }
                    // A property with no column is either calculated -- a recipe's ABV, a
                    // fermentable's colour in SRM, written out but worked out again rather than
                    // read back -- or one of the legacy BeerXML fields that says something about
                    // the record which `crate::legacy` turns into columns that do exist.
                    None => {
                        legacy.insert(property.to_owned(), value);
                    }
                }
            }
        }
    }
    Ok(MappedRecord { table, columns, local_id, folder_path, references, legacy })
}

/// Read every record of a named collection out of a document.
pub fn map_collection(
    format_name: &str,
    document: &Value,
    root: &str,
    record_name: &str,
) -> Result<Vec<MappedRecord>, MappingError> {
    let format =
        format_data().format(format_name).ok_or_else(|| MappingError::UnknownFormat(format_name.to_owned()))?;
    let definition = format.record_named(record_name).ok_or_else(|| MappingError::UnknownRecord {
        format: format_name.to_owned(),
        record: record_name.to_owned(),
    })?;

    let Some(records) = document.get(root).and_then(|node| node.get(record_name)).and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    records.iter().map(|record| map_record_with(format, definition, record)).collect()
}

/// Parse an enum column back into its typed form, for callers that want one.
pub fn parse_enum<T: FromStr>(value: &Value) -> Option<T> {
    value.as_str()?.parse().ok()
}
