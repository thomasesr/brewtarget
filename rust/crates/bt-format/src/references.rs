// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/references.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The checks JSON Schema cannot make.
//!
//! The dotBeer schema says so itself, in the description of its `LocalId` type: a validating
//! program has to check that every defined local ID is unique within the file, that every
//! referred-to one is defined somewhere in it, and that it names the right kind of thing -- the
//! style of a recipe has to be a style.  JSON Schema can only check the shape of the string.
//!
//! Which fields are references, and what each one points at, is not guessed: the schema annotates
//! them with `x-refers-to`, naming the collection the target must live in.  So this walks the
//! schema for those annotations rather than carrying a hand-written list that would drift.

use std::collections::BTreeMap;

use serde_json::Value;

/// A problem that validation cannot see but a reader has to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceError {
    /// Where in the document the problem is, as a JSON pointer.
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Which property names are references, and the collection each points into.
///
/// Read from the `x-refers-to` annotations in the schema files.
#[derive(Debug, Clone, Default)]
pub struct ReferenceRules {
    targets: BTreeMap<String, String>,
}

impl ReferenceRules {
    /// Collect the annotations from every schema file in a directory.
    pub fn from_schema_directory(directory: &std::path::Path) -> std::io::Result<Self> {
        let mut targets = BTreeMap::new();
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if !path.extension().is_some_and(|extension| extension == "schema" || extension == "json") {
                continue;
            }
            let source = std::fs::read_to_string(&path)?;
            let Ok(document) = crate::jsonc::value_from_str(&source) else {
                continue;
            };
            collect_annotations(&document, &mut targets);
        }
        Ok(Self { targets })
    }

    pub fn target_of(&self, property: &str) -> Option<&str> {
        self.targets.get(property).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

/// Walk a schema looking for `{"x-refers-to": "..."}` under a named property.
fn collect_annotations(node: &Value, targets: &mut BTreeMap<String, String>) {
    match node {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for (name, definition) in properties {
                    if let Some(target) = definition.get("x-refers-to").and_then(Value::as_str) {
                        targets.insert(name.clone(), target.to_owned());
                    }
                }
            }
            for value in object.values() {
                collect_annotations(value, targets);
            }
        }
        Value::Array(array) => {
            for value in array {
                collect_annotations(value, targets);
            }
        }
        _ => {}
    }
}

/// Where a local ID was defined: the collection it sits in, and the pointer to the record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// e.g. `/DotBeer/hops`.
    pub collection: String,
    /// e.g. `/DotBeer/hops/0`.
    pub path: String,
}

/// Every local ID defined in a document, and where.
pub fn definitions(document: &Value) -> (BTreeMap<String, Definition>, Vec<ReferenceError>) {
    let mut found: BTreeMap<String, Definition> = BTreeMap::new();
    let mut duplicates = Vec::new();
    walk(document, String::new(), &mut |path, object| {
        let Some(local_id) = object.get("local_id").and_then(Value::as_str) else {
            return;
        };
        // The collection is everything above the record's own index.
        let collection = path.rsplit_once('/').map_or(String::new(), |(parent, _)| parent.to_owned());
        let definition = Definition { collection, path: path.to_owned() };
        if let Some(existing) = found.get(local_id) {
            duplicates.push(ReferenceError {
                path: path.to_owned(),
                message: format!("local ID {local_id:?} is already defined at {}", existing.path),
            });
        } else {
            found.insert(local_id.to_owned(), definition);
        }
    });
    (found, duplicates)
}

/// Check every reference in a document.
///
/// Implements the three rules the schema's own `LocalId` description sets out: local IDs are
/// unique, every reference resolves, and every reference names the right kind of thing.
pub fn check(document: &Value, rules: &ReferenceRules) -> Vec<ReferenceError> {
    let (defined, mut problems) = definitions(document);

    walk(document, String::new(), &mut |path, object| {
        for (name, value) in object {
            let Some(target) = rules.target_of(name) else {
                continue;
            };
            let Some(reference) = value.as_str() else {
                continue;
            };
            let Some(definition) = defined.get(reference) else {
                problems.push(ReferenceError {
                    path: format!("{path}/{name}"),
                    message: format!("local ID {reference:?} is not defined anywhere in this file"),
                });
                continue;
            };
            if !collection_matches(target, &definition.collection) {
                problems.push(ReferenceError {
                    path: format!("{path}/{name}"),
                    message: format!(
                        "{name} must refer to an item in {target}, but {reference:?} is in {}",
                        definition.collection
                    ),
                });
            }
        }
    });

    problems.sort_by(|left, right| left.path.cmp(&right.path).then(left.message.cmp(&right.message)));
    problems
}

/// Whether a definition's collection satisfies a `x-refers-to` target.
///
/// A target may contain `*` for an index, as `/DotBeer/recipes/*/brew_logs` does, since brew logs
/// live inside whichever recipe they belong to.
fn collection_matches(target: &str, collection: &str) -> bool {
    let target_parts: Vec<&str> = target.split('/').collect();
    let collection_parts: Vec<&str> = collection.split('/').collect();
    target_parts.len() == collection_parts.len()
        && target_parts.iter().zip(&collection_parts).all(|(wanted, actual)| *wanted == "*" || wanted == actual)
}

/// Visit every object in a document, giving its JSON pointer.
fn walk(node: &Value, path: String, visit: &mut impl FnMut(&str, &serde_json::Map<String, Value>)) {
    match node {
        Value::Object(object) => {
            visit(&path, object);
            for (name, value) in object {
                walk(value, format!("{path}/{name}"), visit);
            }
        }
        Value::Array(array) => {
            for (index, value) in array.iter().enumerate() {
                walk(value, format!("{path}/{index}"), visit);
            }
        }
        _ => {}
    }
}
