// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/schema.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Validating a document against its JSON schema.
//!
//! Ported from `src/serialization/json/JsonSchema.cpp`.
//!
//! The `.beer` and BeerJSON schemas are split across a directory of files that refer to one
//! another by relative name -- `Culture.beer.schema#/$defs/CultureAmount` and so on -- while each
//! declares an absolute `$id`.  The C++ resolves a reference by treating its URI as a path
//! relative to the schema directory; the same thing happens here, through a retriever that holds
//! every file in the directory under both its `$id` and its bare name.
//!
//! The schema files carry comments of their own, so they are read the same way the documents are.

use std::collections::HashMap;

use jsonschema::{Retrieve, Uri, Validator};
use serde_json::Value;

use crate::jsonc;

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("could not read {path}: {source}")]
    Read { path: String, source: std::io::Error },
    #[error("{path} is not valid JSON: {source}")]
    Parse { path: String, source: serde_json::Error },
    #[error("{0} contains no schema files")]
    NoSchemaFiles(String),
    #[error("could not build the schema: {0}")]
    Build(String),
    /// The document parsed but does not satisfy the schema.  Every failure is listed, because
    /// fixing one at a time is miserable when a hand-written file has several.
    #[error("{} schema violation(s):\n{}", .0.len(), .0.join("\n"))]
    Invalid(Vec<String>),
}

/// Every schema file in a directory, indexed by the names a reference may use.
struct DirectoryRetriever {
    documents: HashMap<String, Value>,
}

impl Retrieve for DirectoryRetriever {
    fn retrieve(&self, uri: &Uri<&str>) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let reference = uri.as_str();
        // A reference may arrive as the absolute `$id`, or as the bare file name it was written
        // as, possibly resolved against some base the crate invented.
        let candidates = [reference.to_owned(), reference.rsplit('/').next().unwrap_or(reference).to_owned()];
        for candidate in candidates {
            if let Some(document) = self.documents.get(&candidate) {
                return Ok(document.clone());
            }
        }
        Err(format!("no schema file for {reference}").into())
    }
}

/// A schema directory, ready to validate documents.
pub struct Schema {
    validator: Validator,
}

impl Schema {
    /// Load every `*.schema` or `*.json` file in `directory` and build a validator for `root`.
    pub fn load(directory: &std::path::Path, root: &str) -> Result<Self, SchemaError> {
        let mut documents = HashMap::new();
        let mut root_document = None;

        let entries = std::fs::read_dir(directory)
            .map_err(|source| SchemaError::Read { path: directory.display().to_string(), source })?;
        for entry in entries {
            let path =
                entry.map_err(|source| SchemaError::Read { path: directory.display().to_string(), source })?.path();
            let is_schema = path.extension().is_some_and(|extension| extension == "schema" || extension == "json");
            if !is_schema {
                continue;
            }
            let name = path.file_name().expect("a file has a name").to_string_lossy().into_owned();
            let source = std::fs::read_to_string(&path)
                .map_err(|source| SchemaError::Read { path: path.display().to_string(), source })?;
            let document: Value = jsonc::value_from_str(&source)
                .map_err(|source| SchemaError::Parse { path: path.display().to_string(), source })?;

            if let Some(id) = document.get("$id").and_then(Value::as_str) {
                documents.insert(id.to_owned(), document.clone());
            }
            if name == root {
                root_document = Some(document.clone());
            }
            documents.insert(name, document);
        }

        if documents.is_empty() {
            return Err(SchemaError::NoSchemaFiles(directory.display().to_string()));
        }
        let root_document =
            root_document.ok_or_else(|| SchemaError::NoSchemaFiles(format!("{root} in {}", directory.display())))?;

        let validator = jsonschema::options()
            .with_retriever(DirectoryRetriever { documents })
            .build(&root_document)
            .map_err(|error| SchemaError::Build(error.to_string()))?;
        Ok(Self { validator })
    }

    pub fn is_valid(&self, document: &Value) -> bool {
        self.validator.is_valid(document)
    }

    /// Validate, reporting every violation rather than only the first.
    pub fn validate(&self, document: &Value) -> Result<(), SchemaError> {
        let failures: Vec<String> =
            self.validator.iter_errors(document).map(|error| format!("  {}: {error}", error.instance_path)).collect();
        if failures.is_empty() { Ok(()) } else { Err(SchemaError::Invalid(failures)) }
    }
}
