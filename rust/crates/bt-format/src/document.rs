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

/// How a nested record relates to the one it was found inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nesting {
    /// A list: the records belong to the one that holds them, and each points back at it.  A
    /// recipe's hop additions, a mash's steps.
    Owned,
    /// A single record the holder refers to.  BeerXML writes a recipe's hop addition and the hop
    /// itself as one element, so the hop has to become a row of its own that the addition then
    /// points at -- the link runs the opposite way to a list.
    Referenced,
}

/// A record together with whatever is nested inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Child {
    /// The field it came from -- `ingredients/hop_additions`, `instructions`, or empty for a
    /// record written into its holder's own element.
    pub path: String,
    pub nesting: Nesting,
    pub records: Vec<Owned>,
}

/// A record together with whatever it owns.
#[derive(Debug, Clone, PartialEq)]
pub struct Owned {
    pub record: MappedRecord,
    /// Records nested inside this one, each saying which way the link runs.
    pub children: Vec<Child>,
}

/// Everything read out of one file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    /// The standalone records, by the **table** they belong to -- `hop`, `fermentable`, and the
    /// rest.  Anything in here may be referred to by local ID.
    ///
    /// Keyed by table rather than by what any one format calls the collection, because the three
    /// formats disagree: a hop is in `hops` in a `.beer` file, `hop_varieties` in BeerJSON and
    /// `<HOPS>` in BeerXML.  Keying by the format that happened to read it meant a document could
    /// only be written back in the format it came from; anything else silently wrote nothing.
    pub collections: Vec<(String, Vec<Owned>)>,
    /// The recipes, which refer to the above and own their additions.
    pub recipes: Vec<Owned>,
}

impl Document {
    /// Every standalone record in the file, flattened.
    pub fn all_records(&self) -> impl Iterator<Item = &Owned> {
        self.collections.iter().flat_map(|(_, records)| records)
    }

    /// The records belonging to one table, if the document has any.
    pub fn records_of(&self, table: &str) -> Option<&[Owned]> {
        self.collections.iter().find(|(name, _)| name == table).map(|(_, records)| records.as_slice())
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

        // The path into the record may be nested -- a recipe's hop additions live under
        // `ingredients/hop_additions` -- or empty, which means the record is written into its
        // holder's own element.  BeerXML does that: a recipe's `<HOP>` is both the addition and
        // the hop, so the hop is read from the very element the addition came from.
        let mut current = node;
        let mut found = true;
        if !field.path.is_empty() {
            for part in field.path.split('/') {
                match current.get(part) {
                    Some(next) => current = next,
                    None => {
                        found = false;
                        break;
                    }
                }
            }
        }
        if !found {
            continue;
        }

        let nested_nodes: Vec<&Value> = match field.field_type.as_str() {
            // A list of exactly one has no array around it in XML, for the same reason the
            // top-level sections do not -- nothing in the document says whether one or many was
            // expected, so the format definition is what decides.  See `records_in`.
            "ListOfRecords" => match current {
                Value::Array(array) => array.iter().collect(),
                Value::Null => Vec::new(),
                single => vec![single],
            },
            // A single nested record, e.g. the ingredient embedded in an addition.
            "Record" => vec![current],
            _ => continue,
        };
        if nested_nodes.is_empty() {
            continue;
        }

        let read: Result<Vec<Owned>, MappingError> =
            nested_nodes.into_iter().map(|nested| read_owned(format, nested_definition, nested)).collect();
        let nesting = match field.field_type.as_str() {
            "ListOfRecords" => Nesting::Owned,
            _ => Nesting::Referenced,
        };
        children.push(Child { path: field.path.clone(), nesting, records: read? });
    }

    // The record may say things in terms the database does not have -- a boil time rather than a
    // boil, a hop "use" rather than a stage -- so the last step of reading one is working out
    // what it meant.  Nothing to do for a format that says it directly.
    let mut owned = Owned { record, children };
    crate::legacy::expand(&mut owned);
    Ok(owned)
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

/// The sections a BeerXML file may contain, as the wrapper element and the record inside it.
///
/// BeerXML nests one level deeper than the JSON formats: a list of hops is `<HOPS>` containing
/// any number of `<HOP>`, where BeerJSON simply has an array.
const BEERXML_SECTIONS: [(&str, &str); 8] = [
    ("FERMENTABLES", "FERMENTABLE"),
    ("HOPS", "HOP"),
    ("MISCS", "MISC"),
    ("YEASTS", "YEAST"),
    ("WATERS", "WATER"),
    ("STYLES", "STYLE"),
    ("EQUIPMENTS", "EQUIPMENT"),
    ("MASHS", "MASH"),
];

/// Read a whole BeerXML document, which must already have been given its synthetic root.
pub fn read_beerxml(document: &Value) -> Result<Document, MappingError> {
    let format = format_data().format("beerXml").ok_or_else(|| MappingError::UnknownFormat("beerXml".to_owned()))?;
    let Some(root_node) = document.get(crate::xml::SYNTHETIC_ROOT) else {
        return Ok(Document::default());
    };

    let mut result = Document::default();
    for (wrapper, record_name) in BEERXML_SECTIONS {
        let Some(definition) = format.record_named(record_name) else { continue };
        let Some(nodes) = records_in(root_node, wrapper, record_name) else { continue };
        let records: Result<Vec<Owned>, MappingError> =
            nodes.iter().map(|node| read_owned(format, definition, node)).collect();
        let Some(table) = bt_model::tables::table_for_cpp_type(&definition.cpp_type) else { continue };
        result.collections.push((table.to_owned(), records?));
    }

    if let Some(definition) = format.record_named("RECIPE")
        && let Some(nodes) = records_in(root_node, "RECIPES", "RECIPE")
    {
        result.recipes = nodes.iter().map(|node| read_owned(format, definition, node)).collect::<Result<_, _>>()?;
    }
    Ok(result)
}

/// The records inside a BeerXML wrapper element.
///
/// A wrapper holding exactly one record has no array around it, since nothing in XML says whether
/// one or many was expected.
fn records_in<'a>(root: &'a Value, wrapper: &str, record_name: &str) -> Option<Vec<&'a Value>> {
    let inner = root.get(wrapper)?.get(record_name)?;
    Some(match inner {
        Value::Array(array) => array.iter().collect(),
        single => vec![single],
    })
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
        // Keyed by the table, not by what this format happens to call the collection, so that a
        // document read from one format can be written in another.
        let Some(table) = bt_model::tables::table_for_cpp_type(&definition.cpp_type) else { continue };
        result.collections.push((table.to_owned(), records?));
    }

    if let Some(definition) = format.record_named(recipe_collection)
        && let Some(nodes) = root_node.get(recipe_collection).and_then(Value::as_array)
    {
        result.recipes = nodes.iter().map(|node| read_owned(format, definition, node)).collect::<Result<_, _>>()?;
    }
    Ok(result)
}

/// Write a record and everything nested inside it.
fn write_owned(format: &Format, definition: &Record, owned: &Owned) -> Value {
    // A format may not have a record for everything this one holds -- BeerXML has no boil -- in
    // which case those children become fields on this record instead.
    let folded = crate::legacy::fold(definition, owned);
    let owned = folded.as_ref().unwrap_or(owned);
    let mut node = crate::writing::write_record_with(format, definition, &owned.record);

    for child in &owned.children {
        let Some(field) = definition.fields.iter().find(|field| field.path == child.path) else {
            continue;
        };
        let Some(nested_type) = field.nested_record_type.as_deref() else {
            continue;
        };
        let Some(nested_definition) =
            format.records.iter().find(|candidate| candidate.name == nested_type || candidate.cpp_type == nested_type)
        else {
            continue;
        };

        let written: Vec<Value> =
            child.records.iter().map(|nested| write_owned(format, nested_definition, nested)).collect();
        match child.nesting {
            // A record written into its holder's own element has no path of its own, so its
            // fields are merged into what has already been written there.
            Nesting::Referenced if child.path.is_empty() => {
                if let (Some(target), Some(Value::Object(source))) = (node.as_object_mut(), written.first()) {
                    for (name, value) in source {
                        target.entry(name.clone()).or_insert_with(|| value.clone());
                    }
                }
            }
            Nesting::Referenced => {
                if let Some(first) = written.into_iter().next() {
                    insert_into(&mut node, &child.path, first);
                }
            }
            Nesting::Owned => insert_into(&mut node, &child.path, Value::Array(written)),
        }
    }
    node
}

/// Put a value at a `/`-separated path within a record.
fn insert_into(node: &mut Value, path: &str, value: Value) {
    let Some(object) = node.as_object_mut() else { return };
    let mut parts = path.split('/').peekable();
    let mut current = object;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            current.insert(part.to_owned(), value);
            return;
        }
        let entry = current.entry(part.to_owned()).or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(serde_json::Map::new());
        }
        current = entry.as_object_mut().expect("just made it an object");
    }
}

/// Write a document back out in one of the formats.
pub fn write(
    format_name: &str,
    document: &Document,
    root: &str,
    collections: &[&str],
    recipe_collection: &str,
) -> Result<Value, MappingError> {
    let format =
        format_data().format(format_name).ok_or_else(|| MappingError::UnknownFormat(format_name.to_owned()))?;

    let mut root_node = serde_json::Map::new();
    // In the order this format lists them, which is the order its schema expects.
    for collection in collections {
        let Some(definition) = format.record_named(collection) else { continue };
        let Some(table) = bt_model::tables::table_for_cpp_type(&definition.cpp_type) else { continue };
        let Some(records) = document.records_of(table) else { continue };
        if records.is_empty() {
            continue;
        }
        let written: Vec<Value> = records.iter().map(|record| write_owned(format, definition, record)).collect();
        root_node.insert((*collection).to_owned(), Value::Array(written));
    }

    if !document.recipes.is_empty()
        && let Some(definition) = format.record_named(recipe_collection)
    {
        let written: Vec<Value> =
            document.recipes.iter().map(|recipe| write_owned(format, definition, recipe)).collect();
        root_node.insert(recipe_collection.to_owned(), Value::Array(written));
    }

    let mut wrapper = serde_json::Map::new();
    wrapper.insert(root.to_owned(), Value::Object(root_node));
    Ok(Value::Object(wrapper))
}

/// Write a `.beer` document.
pub fn write_dotbeer(document: &Document) -> Result<Value, MappingError> {
    write("dotBeer", document, "DotBeer", &DOTBEER_COLLECTIONS, "recipes")
}

/// Write a BeerJSON document.
pub fn write_beerjson(document: &Document) -> Result<Value, MappingError> {
    write("beerJson", document, "beerjson", &BEERJSON_COLLECTIONS, "recipes")
}

/// Write a BeerXML document, ready for [`crate::xml::write`].
///
/// BeerXML nests one level deeper than the JSON formats, so each collection goes inside the
/// wrapper element it is read from: a list of hops is `<HOPS>` holding any number of `<HOP>`.
pub fn write_beerxml(document: &Document) -> Result<Value, MappingError> {
    let format = format_data().format("beerXml").ok_or_else(|| MappingError::UnknownFormat("beerXml".to_owned()))?;
    let mut root_node = serde_json::Map::new();

    for (wrapper, record_name) in BEERXML_SECTIONS {
        let Some(definition) = format.record_named(record_name) else { continue };
        let Some(table) = bt_model::tables::table_for_cpp_type(&definition.cpp_type) else { continue };
        let Some(records) = document.records_of(table) else { continue };
        if records.is_empty() {
            continue;
        }
        let written: Vec<Value> = records.iter().map(|record| write_owned(format, definition, record)).collect();
        let mut section = serde_json::Map::new();
        section.insert(record_name.to_owned(), Value::Array(written));
        root_node.insert(wrapper.to_owned(), Value::Object(section));
    }

    if !document.recipes.is_empty()
        && let Some(definition) = format.record_named("RECIPE")
    {
        let written: Vec<Value> =
            document.recipes.iter().map(|recipe| write_owned(format, definition, recipe)).collect();
        let mut section = serde_json::Map::new();
        section.insert("RECIPE".to_owned(), Value::Array(written));
        root_node.insert("RECIPES".to_owned(), Value::Object(section));
    }

    let mut wrapper = serde_json::Map::new();
    wrapper.insert(crate::xml::SYNTHETIC_ROOT.to_owned(), Value::Object(root_node));
    Ok(Value::Object(wrapper))
}
