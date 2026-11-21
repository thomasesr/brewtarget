// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/document.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! A whole file, read into rows that are ready to store.
//!
//! The ingredient collections are flat, but a recipe is not: it refers to a style, an equipment,
//! a mash, a boil and a fermentation by local ID, and it owns its additions, instructions and
//! brew logs outright.  Those owned records are rows of their own with the recipe's key in them,
//! which cannot be filled in until the recipe has been stored and has a key.
//!
//! So this produces the graph rather than trying to flatten it: every record, what it refers to
//! by local ID, and what it owns.  Turning local IDs into foreign keys is the storing layer's
//! job, since only it knows what key anything was given.

use serde_json::Value;

use crate::data::{Format, Record, format_data};
use crate::mapping::{MappedRecord, MappingError, map_record_with};

/// A record together with whatever it owns.
#[derive(Debug, Clone, PartialEq)]
pub struct Owned {
    pub record: MappedRecord,
    /// Records this one owns, by the field they came from -- `ingredients/hop_additions`,
    /// `instructions` and so on.  Each child carries the references it makes of its own.
    pub children: Vec<(String, Vec<Owned>)>,
}

/// Everything read out of one file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// The standalone records, by the collection they came from: `hops`, `fermentables`, and the
    /// rest.  Anything in here may be referred to by local ID.
    pub collections: Vec<(String, Vec<Owned>)>,
    /// The recipes, which refer to the above and own their additions.
    pub recipes: Vec<Owned>,
}

impl Document {
    /// Every standalone record in the file, flattened.
    pub fn all_records(&self) -> impl Iterator<Item = &Owned> {
        self.collections.iter().flat_map(|(_, records)| records)
    }

    /// Look up a record by the local ID it declares.
    pub fn by_local_id(&self, local_id: &str) -> Option<&Owned> {
        self.all_records().find(|owned| owned.record.local_id.as_deref() == Some(local_id))
    }

    pub fn len(&self) -> usize {
        self.collections.iter().map(|(_, records)| records.len()).sum::<usize>() + self.recipes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Read a record and everything nested inside it.
fn read_owned(format: &Format, definition: &Record, node: &Value) -> Result<Owned, MappingError> {
    let record = map_record_with(format, definition, node)?;

    let mut children = Vec::new();
    for field in &definition.fields {
        let Some(nested_type) = field.nested_record_type.as_deref() else {
            continue;
        };
        let Some(nested_definition) =
            format.records.iter().find(|candidate| candidate.name == nested_type || candidate.cpp_type == nested_type)
        else {
            continue;
        };

        // The path into the record may be nested: a recipe's hop additions live under
        // `ingredients/hop_additions`.
        let mut current = node;
        let mut found = true;
        for part in field.path.split('/') {
            match current.get(part) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }

        let nested_nodes: Vec<&Value> = match field.field_type.as_str() {
            "ListOfRecords" => current.as_array().map(|array| array.iter().collect()).unwrap_or_default(),
            // A single nested record, e.g. the ingredient embedded in an addition.
            "Record" => vec![current],
            _ => continue,
        };
        if nested_nodes.is_empty() {
            continue;
        }

        let read: Result<Vec<Owned>, MappingError> =
            nested_nodes.into_iter().map(|nested| read_owned(format, nested_definition, nested)).collect();
        children.push((field.path.clone(), read?));
    }

    Ok(Owned { record, children })
}

/// The collections a `.beer` file may contain, in the order they have to be stored.
///
/// Order matters: a recipe refers to a style and a mash, so those have to exist first.  Within
/// the ingredients there are no references between collections, so their order is only for
/// readability.
const DOTBEER_COLLECTIONS: [&str; 10] =
    ["fermentables", "hops", "miscs", "cultures", "waters", "styles", "equipments", "mashes", "boils", "fermentations"];

/// Read a whole `.beer` document.
pub fn read_dotbeer(document: &Value) -> Result<Document, MappingError> {
    read("dotBeer", document, "DotBeer", &DOTBEER_COLLECTIONS, "recipes")
}

/// The collections a BeerJSON file may contain.
///
/// BeerJSON names several of them differently -- hops are `hop_varieties`, miscs are
/// `miscellaneous_ingredients`, waters are `profiles` -- which is exactly the sort of thing the
/// extracted record definitions exist so that nobody has to remember.
const BEERJSON_COLLECTIONS: [&str; 10] = [
    "fermentables",
    "hop_varieties",
    "miscellaneous_ingredients",
    "cultures",
    "profiles",
    "styles",
    "equipments",
    "mashes",
    "boils",
    "fermentations",
];

/// Read a whole BeerJSON document.
pub fn read_beerjson(document: &Value) -> Result<Document, MappingError> {
    read("beerJson", document, "beerjson", &BEERJSON_COLLECTIONS, "recipes")
}

/// Read a document of any of the formats.
pub fn read(
    format_name: &str,
    document: &Value,
    root: &str,
    collections: &[&str],
    recipe_collection: &str,
) -> Result<Document, MappingError> {
    let format =
        format_data().format(format_name).ok_or_else(|| MappingError::UnknownFormat(format_name.to_owned()))?;
    let Some(root_node) = document.get(root) else {
        return Ok(Document::default());
    };

    let mut result = Document::default();
    for collection in collections {
        let Some(definition) = format.record_named(collection) else {
            continue;
        };
        let Some(nodes) = root_node.get(collection).and_then(Value::as_array) else {
            continue;
        };
        let records: Result<Vec<Owned>, MappingError> =
            nodes.iter().map(|node| read_owned(format, definition, node)).collect();
        result.collections.push(((*collection).to_owned(), records?));
    }

    if let Some(definition) = format.record_named(recipe_collection)
        && let Some(nodes) = root_node.get(recipe_collection).and_then(Value::as_array)
    {
        result.recipes = nodes.iter().map(|node| read_owned(format, definition, node)).collect::<Result<_, _>>()?;
    }
    Ok(result)
}
