// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/writing.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Writing records back out.
//!
//! The same field mappings as [`crate::mapping`], run the other way: a column-keyed row becomes
//! the shape the format expects, with measurements given a unit again.
//!
//! # Which unit a measurement is written in
//!
//! The C++ takes the unit from `JsonMeasureableUnitsMapping::defaultUnit`, which returns
//! `nameToUnit.cbegin()->second`.  That container is a `std::map` keyed by the unit's name, so
//! "first" means alphabetically first rather than anything chosen: for mass that happens to be
//! "g", but for volume it is "bbl", and the C++ therefore writes volumes in US barrels.  A 22
//! litre batch comes out as 0.1875 bbl.
//!
//! That reads as an accident of container choice rather than a decision, and it is not one worth
//! reproducing: it makes exported files awkward for people and surprising for other software,
//! while being no more correct, since the value is exact either way.  So a measurement is written
//! in the canonical unit for whatever it measures -- kilograms, litres -- which is what the
//! column already holds and what round-trips without conversion.

use serde_json::{Map, Value};

use crate::data::{Field, Format, Record, format_data};
use crate::mapping::MappingError;

/// Put a value at a `/`-separated path, creating the objects on the way.
fn insert_at(target: &mut Map<String, Value>, path: &str, value: Value) {
    let mut parts = path.split('/').peekable();
    let mut current = target;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            current.insert(part.to_owned(), value);
            return;
        }
        let entry = current.entry(part.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry.as_object_mut().expect("just made it an object");
    }
}

/// The name this format gives our canonical unit for a quantity.
///
/// A mass column holds kilograms, so the file needs whichever name the format's table maps onto
/// kilograms -- "kg" for both BeerJSON and dotBeer.
fn canonical_unit_name<'a>(format: &'a Format, mapping_name: &str, unit: bt_measure::Unit) -> Option<&'a str> {
    match format.unit_mappings.get(mapping_name)? {
        crate::data::UnitMapping::Units { units } => units
            .iter()
            .find(|(_, our_name)| bt_measure::Unit::by_name(our_name) == Some(unit))
            .map(|(their_name, _)| their_name.as_str()),
        crate::data::UnitMapping::SingleUnit { names } => names.first().map(String::as_str),
        crate::data::UnitMapping::AnyOf { mappings } => {
            mappings.iter().find_map(|mapping| canonical_unit_name(format, mapping, unit))
        }
    }
}

/// Write one field, if the row has anything to say about it.
fn write_field(
    format: &Format,
    table: &str,
    columns: &Map<String, Value>,
    field: &Field,
    target: &mut Map<String, Value>,
) {
    // A composite amount is two columns and one field, so it is handled before the rest.
    if field.property.as_deref().is_some_and(|property| property.ends_with("::amount")) {
        let quantity = bt_model::tables::column_for_property(table, "IngredientAmount::quantity")
            .and_then(|column| columns.get(column))
            .and_then(Value::as_f64);
        let unit = bt_model::tables::column_for_property(table, "IngredientAmount::unit")
            .and_then(|column| columns.get(column))
            .and_then(Value::as_str)
            .and_then(bt_measure::Unit::by_name);
        if let (Some(quantity), Some(unit), Some(mapping)) = (quantity, unit, field.decoder.as_deref())
            && let Some(unit_name) = canonical_unit_name(format, mapping, unit)
        {
            // An amount is stored in whatever unit the recipe used, so it goes back out in that
            // same unit rather than being converted to anything.
            let mut measurement = Map::new();
            measurement.insert("unit".to_owned(), Value::String(unit_name.to_owned()));
            measurement.insert("value".to_owned(), Value::from(quantity));
            insert_at(target, &field.path, Value::Object(measurement));
        }
        return;
    }

    let Some(property) = field.property.as_deref() else { return };
    let Some(column) = bt_model::tables::column_for_property(table, property) else { return };
    let Some(value) = columns.get(column) else { return };
    if value.is_null() {
        return;
    }

    match field.field_type.as_str() {
        "String" | "Enum" | "Date" | "DateTimeOffset" | "DateOrDateTimeLocal" | "Bool" | "Int" | "UInt" | "Double" => {
            insert_at(target, &field.path, value.clone());
        }
        "MeasurementWithUnits" | "OneOfMeasurementsWithUnits" => {
            let Some(quantity) = value.as_f64() else { return };
            let Some(mapping) = field.decoder.as_deref() else { return };
            // The column holds the canonical unit for whatever it measures, so that is the one
            // named -- see the note at the top of this file about what the C++ does instead.
            let Some(unit) = canonical_unit_for(format, mapping) else { return };
            let Some(unit_name) = canonical_unit_name(format, mapping, unit) else { return };
            let mut measurement = Map::new();
            measurement.insert("unit".to_owned(), Value::String(unit_name.to_owned()));
            measurement.insert("value".to_owned(), Value::from(quantity));
            insert_at(target, &field.path, Value::Object(measurement));
        }
        "SingleUnitValue" => {
            let Some(quantity) = value.as_f64() else { return };
            let unit_name = field.decoder.as_deref().and_then(|mapping| match format.unit_mappings.get(mapping) {
                Some(crate::data::UnitMapping::SingleUnit { names }) => names.first().map(String::as_str),
                _ => None,
            });
            match unit_name {
                Some(name) => {
                    let mut measurement = Map::new();
                    measurement.insert("unit".to_owned(), Value::String(name.to_owned()));
                    measurement.insert("value".to_owned(), Value::from(quantity));
                    insert_at(target, &field.path, Value::Object(measurement));
                }
                None => insert_at(target, &field.path, Value::from(quantity)),
            }
        }
        // Structural, or not stored: the caller deals with nested records, and a local ID or
        // folder path is supplied separately because neither is a column.
        _ => {}
    }
}

/// The canonical unit a measurement table measures in.
fn canonical_unit_for(format: &Format, mapping_name: &str) -> Option<bt_measure::Unit> {
    match format.unit_mappings.get(mapping_name)? {
        crate::data::UnitMapping::Units { units } => {
            units.values().find_map(|name| bt_measure::Unit::by_name(name)).map(|unit| unit.canonical())
        }
        crate::data::UnitMapping::SingleUnit { .. } => None,
        crate::data::UnitMapping::AnyOf { mappings } => {
            mappings.iter().find_map(|mapping| canonical_unit_for(format, mapping))
        }
    }
}

/// Write a row back out in the shape the format expects.
///
/// `local_id` and `folder_path` are passed in rather than read from the row, because neither is
/// a column: one exists only within a file and the other is a folder row in the database.
pub fn write_record(
    format_name: &str,
    record_name: &str,
    table: &str,
    columns: &Map<String, Value>,
    local_id: Option<&str>,
    folder_path: Option<&str>,
) -> Result<Value, MappingError> {
    let format =
        format_data().format(format_name).ok_or_else(|| MappingError::UnknownFormat(format_name.to_owned()))?;
    let definition = format.record_named(record_name).ok_or_else(|| MappingError::UnknownRecord {
        format: format_name.to_owned(),
        record: record_name.to_owned(),
    })?;
    Ok(write_record_with(format, definition, table, columns, local_id, folder_path))
}

/// Write a row against a definition already in hand.
pub fn write_record_with(
    format: &Format,
    definition: &Record,
    table: &str,
    columns: &Map<String, Value>,
    local_id: Option<&str>,
    folder_path: Option<&str>,
) -> Value {
    let mut target = Map::new();
    for field in &definition.fields {
        match field.field_type.as_str() {
            "LocalId" if field.path == "local_id" => {
                if let Some(local_id) = local_id {
                    target.insert("local_id".to_owned(), Value::String(local_id.to_owned()));
                }
            }
            "FolderPath" => {
                if let Some(folder_path) = folder_path {
                    insert_at(&mut target, &field.path, Value::String(folder_path.to_owned()));
                }
            }
            _ => write_field(format, table, columns, field, &mut target),
        }
    }
    Value::Object(target)
}
