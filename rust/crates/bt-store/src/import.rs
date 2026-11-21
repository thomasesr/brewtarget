// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/import.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Storing what a file said.
//!
//! `bt-format` reads a file into a graph whose edges are local IDs -- identifiers that mean
//! something inside that file and nothing outside it.  Turning those into foreign keys is this
//! layer's job, because only it knows what key anything was actually given.
//!
//! Records are stored in dependency order: the things a recipe refers to first, then the recipes,
//! then the rows a recipe owns, each carrying the recipe's key.  Folder paths become folder rows
//! along the way, since a record cannot point at a folder that does not exist yet.

use std::collections::BTreeMap;

use bt_format::document::{Document, Owned};
use bt_format::mapping::MappedRecord;
use serde_json::{Map, Value};

use crate::{Store, StoreError, folders, rows};

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Imported {
    /// The key each local ID was given, and the table it went into.
    pub by_local_id: BTreeMap<String, (String, i64)>,
    /// The recipes stored, by their key.
    pub recipe_ids: Vec<i64>,
    /// How many rows were written in total, children included.
    pub rows_written: usize,
}

/// Which model property a reference field feeds, so it can be turned into the right column.
///
/// A file says `style_id`; the database says `recipe.style_id`.  The names happen to agree for
/// most of them, which is not something to rely on -- the mapping is what the format definition
/// says, and the reader hands over the field name it saw.
fn column_for_reference(table: &str, field: &str) -> Option<&'static str> {
    bt_model::tables::columns_of(table)?.iter().copied().find(|column| *column == field)
}

/// The column a child row uses to point back at what owns it.
fn owner_column(child_table: &str, owner_table: &str) -> Option<&'static str> {
    let candidate = format!("{owner_table}_id");
    bt_model::tables::columns_of(child_table)?.iter().copied().find(|column| *column == candidate)
}

/// Store one record, resolving its folder and its references first.
async fn store_record(
    store: &Store,
    record: &MappedRecord,
    resolved: &BTreeMap<String, (String, i64)>,
    extra: &[(&str, i64)],
) -> Result<i64, StoreError> {
    let mut columns: Map<String, Value> = record.columns.clone();

    // A folder path is a path, and the database holds a tree, so the chain has to be created
    // before anything can point into it.  Only the tables that have a folder column take part:
    // a mash step is not filed anywhere, its mash is.
    if let Some(path) = &record.folder_path
        && let Some(column) = column_for_reference(record.table, "contained_in_folder_id")
        && folders::folder_table_for(record.table).is_ok()
        && let Some(folder_id) = folders::ensure_path(store, record.table, path).await?
    {
        columns.insert(column.to_owned(), Value::from(folder_id));
    }

    for (field, local_id) in &record.references {
        let Some((_, key)) = resolved.get(local_id) else {
            // A reference that does not resolve was already reported by the format's own checks,
            // so the row is stored without it rather than the whole import failing.
            continue;
        };
        if let Some(column) = column_for_reference(record.table, field) {
            columns.insert(column.to_owned(), Value::from(*key));
        }
    }

    for (column, key) in extra {
        columns.insert((*column).to_owned(), Value::from(*key));
    }

    rows::insert(store, record.table, &columns).await
}

/// Store a record and everything it owns.
async fn store_owned(
    store: &Store,
    owned: &Owned,
    resolved: &mut BTreeMap<String, (String, i64)>,
    extra: &[(&str, i64)],
    written: &mut usize,
) -> Result<i64, StoreError> {
    let key = store_record(store, &owned.record, resolved, extra).await?;
    *written += 1;
    if let Some(local_id) = &owned.record.local_id {
        resolved.insert(local_id.clone(), (owned.record.table.to_owned(), key));
    }

    for (_, children) in &owned.children {
        for child in children {
            // A child points back at whatever owns it, which is the only thing it needs that is
            // not in the file: `mash_step.mash_id`, `hop_in_recipe.recipe_id`.
            let link = owner_column(child.record.table, owned.record.table).map(|column| (column, key));
            let extra: Vec<(&str, i64)> = link.into_iter().collect();
            Box::pin(store_owned(store, child, resolved, &extra, written)).await?;
        }
    }
    Ok(key)
}

/// Store everything a file described.
///
/// The order is the dependency order: the things a recipe refers to go first, so that by the time
/// a recipe is stored every local ID it names already has a key.
pub async fn import(store: &Store, document: &Document) -> Result<Imported, StoreError> {
    let mut resolved: BTreeMap<String, (String, i64)> = BTreeMap::new();
    let mut summary = Imported::default();

    for (_, records) in &document.collections {
        for record in records {
            store_owned(store, record, &mut resolved, &[], &mut summary.rows_written).await?;
        }
    }

    for recipe in &document.recipes {
        let key = store_owned(store, recipe, &mut resolved, &[], &mut summary.rows_written).await?;
        summary.recipe_ids.push(key);
    }

    summary.by_local_id = resolved;
    Ok(summary)
}
