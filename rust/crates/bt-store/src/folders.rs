// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/folders.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading and creating folders.
//!
//! Each kind of thing keeps its own folder table -- `folder_for_recipe`, `folder_for_hop` and so
//! on, eleven in all, identical in shape.  They are addressed here by name, so the name is
//! checked against the generated list first: it reaches SQL as text, and a table name cannot be
//! bound as a parameter.

use bt_model::folder::{FolderNode, FolderTree};
use bt_model::tables::TABLES;
use sqlx::Row;

use crate::{Store, StoreError, dispatch};

/// The folder table for a kind of thing, e.g. `recipe` gives `folder_for_recipe`.
pub fn folder_table_for(entity: &str) -> Result<&'static str, StoreError> {
    let wanted = format!("folder_for_{entity}");
    TABLES.iter().copied().find(|table| *table == wanted).ok_or(StoreError::UnknownFolderTable(wanted))
}

/// Read every folder of one kind into a tree.
pub async fn load_tree(store: &Store, entity: &str) -> Result<FolderTree, StoreError> {
    let table = folder_table_for(entity)?;
    let sql = format!("SELECT id, name, deleted, contained_in_folder_id FROM {table}");
    // Rows are turned into FolderNodes inside the dispatch, because each backend has its own row
    // type and the arms have to agree on what they produce.
    let nodes: Vec<FolderNode> = dispatch!(store, |pool| {
        sqlx::query(&sql)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| {
                Ok(FolderNode {
                    id: row.try_get("id")?,
                    name: row.try_get::<Option<String>, _>("name")?.unwrap_or_default(),
                    parent_id: row.try_get("contained_in_folder_id")?,
                    deleted: row.try_get::<Option<bool>, _>("deleted")?.unwrap_or(false),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?
    });
    Ok(FolderTree::new(nodes)?)
}

/// Find the folder at `path`, creating it and any missing folder above it.
///
/// Ported from `Folder::ensure`.  An empty path means the root, which is no folder at all rather
/// than a folder named "".
pub async fn ensure_path(store: &Store, entity: &str, path: &str) -> Result<Option<i64>, StoreError> {
    let table = folder_table_for(entity)?;
    let normalised = bt_model::folder::normalise(path);
    if normalised.is_empty() {
        return Ok(None);
    }

    let mut tree = load_tree(store, entity).await?;
    let mut parent_id: Option<i64> = None;
    let mut walked = String::new();

    for component in normalised.split('/') {
        walked = if walked.is_empty() { component.to_owned() } else { format!("{walked}/{component}") };
        match tree.find_by_path(&walked) {
            Some(existing) => parent_id = Some(existing.id),
            None => {
                let insert = store.placeholders(&format!(
                    "INSERT INTO {table} (name, deleted, contained_in_folder_id) VALUES (?, ?, ?) RETURNING id"
                ));
                let id: i64 = dispatch!(store, |pool| {
                    sqlx::query_scalar(&insert).bind(component).bind(false).bind(parent_id).fetch_one(pool).await?
                });
                // Keep the tree in step so the next component can find what was just created.
                let mut nodes: Vec<FolderNode> = tree.iter().cloned().collect();
                nodes.push(FolderNode { id, name: component.to_owned(), parent_id, deleted: false });
                tree = FolderTree::new(nodes)?;
                parent_id = Some(id);
            }
        }
    }
    Ok(parent_id)
}

/// The full path of one folder, or `None` if there is no such folder.
pub async fn full_path(store: &Store, entity: &str, id: i64) -> Result<Option<String>, StoreError> {
    Ok(load_tree(store, entity).await?.full_path(id))
}
