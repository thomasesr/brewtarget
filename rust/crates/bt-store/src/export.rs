// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/export.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading a database back out into the graph a file describes.
//!
//! The reverse of [`crate::import`].  Foreign keys become local identifiers again, because that
//! is what a file can carry: a key means nothing outside the database it came from, so every
//! record that something refers to is given a name made from its table and key.
//!
//! Rows marked deleted are left out.  They are kept in the database so that recipes using them
//! still resolve, but a file is a thing to hand to somebody and should not carry them.

use std::collections::BTreeMap;

use bt_format::document::{Child, Document, Nesting, Owned};
use bt_format::mapping::MappedRecord;
use serde_json::{Map, Value};

use crate::{Store, StoreError, dispatch, folders};

/// The collections a file carries, and the table each comes from.
const COLLECTIONS: [(&str, &str); 10] = [
    ("fermentables", "fermentable"),
    ("hops", "hop"),
    ("miscs", "misc"),
    ("cultures", "yeast"),
    ("waters", "water"),
    ("styles", "style"),
    ("equipments", "equipment"),
    ("mashes", "mash"),
    ("boils", "boil"),
    ("fermentations", "fermentation"),
];

/// What a record owns, and the field a file writes it under.
const OWNED: [(&str, &str, &str, &str); 3] = [
    ("mash", "mash_step", "mash_id", "mash_steps"),
    ("boil", "boil_step", "boil_id", "boil_steps"),
    ("fermentation", "fermentation_step", "fermentation_id", "fermentation_steps"),
];

/// What a recipe owns, and the field a file writes it under.
const RECIPE_OWNED: [(&str, &str); 6] = [
    ("fermentable_in_recipe", "ingredients/fermentable_additions"),
    ("hop_in_recipe", "ingredients/hop_additions"),
    ("misc_in_recipe", "ingredients/misc_additions"),
    ("yeast_in_recipe", "ingredients/culture_additions"),
    ("instruction", "instructions"),
    ("brew_log", "brew_logs"),
];

/// The name a record goes by inside a file.
///
/// Made from the table and the key so that it is unique across the whole file, which is what the
/// format requires even between records of different kinds.
fn local_id(table: &str, key: i64) -> String {
    format!("{table}_{key}")
}

/// Read every row of a table that is not marked deleted.
async fn rows_of(store: &Store, table: &str) -> Result<Vec<Map<String, Value>>, StoreError> {
    let columns = bt_model::tables::columns_of(table).ok_or_else(|| StoreError::UnknownTable(table.to_owned()))?;
    let has_deleted = columns.contains(&"deleted");
    let sql = if has_deleted {
        format!("SELECT * FROM {table} WHERE deleted IS NULL OR deleted = FALSE")
    } else {
        format!("SELECT * FROM {table}")
    };

    // Rows are read as JSON so that the same column map the reader produces can be handed to the
    // writer, which already knows what every column means.
    let found: Vec<Map<String, Value>> = dispatch!(store, |pool| {
        use sqlx::{Column, Row, ValueRef};
        sqlx::query(&sql)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|row| {
                let mut map = Map::new();
                for (index, column) in row.columns().iter().enumerate() {
                    let raw = row.try_get_raw(index)?;
                    if raw.is_null() {
                        continue;
                    }
                    // What the column means decides how it is read.  A BOOLEAN is a number on
                    // SQLite and a TINYINT on MySQL, so asking the database what it has would
                    // turn every flag into a 1 and nothing would read the file back.
                    let value = if bt_model::tables::is_boolean_column(table, column.name()) {
                        row.try_get::<Option<bool>, _>(index)
                            .ok()
                            .flatten()
                            .or_else(|| row.try_get::<Option<i64>, _>(index).ok().flatten().map(|flag| flag != 0))
                            .map(Value::from)
                    } else {
                        row.try_get::<Option<f64>, _>(index)
                            .ok()
                            .flatten()
                            .map(Value::from)
                            .or_else(|| row.try_get::<Option<i64>, _>(index).ok().flatten().map(Value::from))
                            .or_else(|| row.try_get::<Option<String>, _>(index).ok().flatten().map(Value::from))
                    };
                    if let Some(value) = value {
                        map.insert(column.name().to_owned(), value);
                    }
                }
                Ok(map)
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?
    });
    Ok(found)
}

/// The key a column holds, if it holds one.
fn key_in(columns: &Map<String, Value>, column: &str) -> Option<i64> {
    columns.get(column)?.as_i64()
}

/// Turn a row into a record, naming what it refers to and where it is filed.
async fn to_record(
    store: &Store,
    table: &'static str,
    mut columns: Map<String, Value>,
    references: &[(&str, &str)],
    folder_trees: &mut BTreeMap<String, bt_model::FolderTree>,
) -> Result<MappedRecord, StoreError> {
    let key = key_in(&columns, "id").unwrap_or_default();

    // A folder is a row here and a path in a file.
    let mut folder_path = None;
    if let Some(folder_id) = key_in(&columns, "contained_in_folder_id")
        && folders::folder_table_for(table).is_ok()
    {
        let tree = match folder_trees.get(table) {
            Some(tree) => tree,
            None => {
                let tree = folders::load_tree(store, table).await?;
                folder_trees.entry(table.to_owned()).or_insert(tree)
            }
        };
        folder_path = tree.full_path(folder_id);
    }

    // A foreign key means nothing outside this database, so it becomes a name.
    let mut named = BTreeMap::new();
    for (column, target_table) in references {
        if let Some(target) = key_in(&columns, column) {
            named.insert((*column).to_owned(), local_id(target_table, target));
        }
        columns.remove(*column);
    }
    columns.remove("contained_in_folder_id");

    Ok(MappedRecord {
        table,
        columns,
        local_id: Some(local_id(table, key)),
        folder_path,
        references: named,
        // Nothing to carry: what is written out comes from columns, and a legacy field is a
        // reading concern -- the writers put a boil time back where BeerXML expects it from the
        // boil's own record.
        legacy: BTreeMap::new(),
    })
}

/// Read everything in a store into the graph a file describes.
pub async fn export(store: &Store) -> Result<Document, StoreError> {
    let mut document = Document::default();
    let mut folder_trees = BTreeMap::new();

    for (collection, table) in COLLECTIONS {
        let mut records = Vec::new();
        for columns in rows_of(store, table).await? {
            let key = key_in(&columns, "id").unwrap_or_default();
            let record = to_record(store, table_name(table), columns, &[], &mut folder_trees).await?;

            // A mash, boil or fermentation carries its steps.
            let mut children = Vec::new();
            if let Some((_, step_table, step_column, field)) = OWNED.iter().find(|(owner, ..)| *owner == table) {
                let mut steps = Vec::new();
                for step_columns in rows_of(store, step_table).await? {
                    if key_in(&step_columns, step_column) != Some(key) {
                        continue;
                    }
                    let step = to_record(store, table_name(step_table), step_columns, &[], &mut folder_trees).await?;
                    steps.push(Owned { record: step, children: Vec::new() });
                }
                if !steps.is_empty() {
                    children.push(Child { path: (*field).to_owned(), nesting: Nesting::Owned, records: steps });
                }
            }
            records.push(Owned { record, children });
        }
        if !records.is_empty() {
            document.collections.push((collection.to_owned(), records));
        }
    }

    // A recipe names what it uses and owns its additions.
    let recipe_references: [(&str, &str); 7] = [
        ("style_id", "style"),
        ("equipment_id", "equipment"),
        ("mash_id", "mash"),
        ("boil_id", "boil"),
        ("fermentation_id", "fermentation"),
        ("water_base", "water"),
        ("water_target", "water"),
    ];
    for columns in rows_of(store, "recipe").await? {
        let key = key_in(&columns, "id").unwrap_or_default();
        let record = to_record(store, "recipe", columns, &recipe_references, &mut folder_trees).await?;

        let mut children = Vec::new();
        for (child_table, field) in RECIPE_OWNED {
            let ingredient = ingredient_reference(child_table);
            let mut additions = Vec::new();
            for child_columns in rows_of(store, child_table).await? {
                if key_in(&child_columns, "recipe_id") != Some(key) {
                    continue;
                }
                let mut child_columns = child_columns;
                child_columns.remove("recipe_id");
                let references: Vec<(&str, &str)> = ingredient.into_iter().collect();
                let addition =
                    to_record(store, table_name(child_table), child_columns, &references, &mut folder_trees).await?;
                additions.push(Owned { record: addition, children: Vec::new() });
            }
            if !additions.is_empty() {
                children.push(Child { path: field.to_owned(), nesting: Nesting::Owned, records: additions });
            }
        }
        document.recipes.push(Owned { record, children });
    }
    Ok(document)
}

/// Which column of an addition names the ingredient it adds.
fn ingredient_reference(child_table: &str) -> Option<(&'static str, &'static str)> {
    match child_table {
        "fermentable_in_recipe" => Some(("fermentable_id", "fermentable")),
        "hop_in_recipe" => Some(("hop_id", "hop")),
        "misc_in_recipe" => Some(("misc_id", "misc")),
        "yeast_in_recipe" => Some(("yeast_id", "yeast")),
        _ => None,
    }
}

/// The generated table list holds the names with a static lifetime, which a record needs.
fn table_name(table: &str) -> &'static str {
    bt_model::tables::TABLES.iter().copied().find(|known| *known == table).unwrap_or("")
}
