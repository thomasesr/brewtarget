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
    pub local_id: Option<String>,
    /// The folder this record says it lives in, which has to become a folder row of its own.
    pub folder_path: Option<String>,
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
        "String" | "Date" | "DateTimeOffset" | "DateOrDateTimeLocal" | "LocalId" | "FolderPath" => {
            Value::String(raw.as_str().ok_or_else(|| bad("expected a string"))?.to_owned())
        }
        "Bool" => Value::Bool(raw.as_bool().ok_or_else(|| bad("expected true or false"))?),
        "Int" | "UInt" => Value::from(raw.as_i64().ok_or_else(|| bad("expected a whole number"))?),
        "Double" => Value::from(raw.as_f64().ok_or_else(|| bad("expected a number"))?),

        // An enum arrives as the string the format uses for it.  Those strings are the ones the
        // database stores too, so they go straight through; it is the schema's job to have
        // rejected any the mapping does not know.
        "Enum" => Value::String(raw.as_str().ok_or_else(|| bad("expected a string"))?.to_owned()),

        // A measurement is a value and a unit together, and is stored canonically: 5 gal becomes
        // 18.927 litres, because that is what the column means.
        "MeasurementWithUnits" | "OneOfMeasurementsWithUnits" => {
            let quantity = raw.get("value").and_then(Value::as_f64).ok_or_else(|| bad("expected a value"))?;
            let unit_name = raw.get("unit").and_then(Value::as_str).ok_or_else(|| bad("expected a unit"))?;
            let mapping = field.decoder.as_deref().ok_or_else(|| bad("no unit table for this measurement"))?;
            let unit = unit_named(format, mapping, unit_name)
                .ok_or_else(|| bad(&format!("{unit_name:?} is not a unit {mapping} knows")))?;
            Value::from(unit.to_canonical(quantity).quantity)
        }

        // A single-unit field is always in the one unit, so only the number is interesting.
        "SingleUnitValue" => {
            let quantity = raw
                .get("value")
                .and_then(Value::as_f64)
                .or_else(|| raw.as_f64())
                .ok_or_else(|| bad("expected a value"))?;
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

    for field in &definition.fields {
        let Some(value) = read_field(format, record, field)? else {
            continue;
        };
        match field.field_type.as_str() {
            // Neither of these is a column: a local ID exists only within the file, and a folder
            // path has to become a folder row before anything can point at it.
            "LocalId" => local_id = value.as_str().map(str::to_owned),
            "FolderPath" => folder_path = value.as_str().map(str::to_owned),
            _ => {
                let Some(property) = field.property.as_deref() else { continue };
                // A property the schema does not store is a calculated one -- a recipe's ABV, a
                // fermentable's colour in SRM -- and is written out but never read back in.
                if let Some(column) = bt_model::tables::column_for_property(table, property) {
                    columns.insert(column.to_owned(), value);
                }
            }
        }
    }
    Ok(MappedRecord { table, columns, local_id, folder_path })
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
