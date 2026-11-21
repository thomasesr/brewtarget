// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/migrations/folders.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Schema v19 to v20: folders stop being path strings and become real rows.
//!
//! Ported from `migrate_to_20` in `src/database/DatabaseSchemaHelper.cpp`.  It is hand-written
//! rather than lifted into `schema/migrations.json` because the middle phase cannot be expressed
//! in SQL: turning `"foo/bar/hum"` into a chain of parent references means materialising the
//! ancestor folders that no row referred to directly.
//!
//! Each kind of thing keeps its own folder table, upholding the design rule that a folder holds
//! one type of thing -- you cannot put a Hop in a Fermentable folder.

use std::collections::BTreeMap;

use sqlx::{AnyConnection, Row};

use super::render::{bind_placeholders, render};
use super::run::MigrationError;
use crate::dialect::Dialect;

/// The tables that had a `folder` path column at schema version 19.
const BASE_TABLES: [&str; 12] = [
    "recipe",
    "style",
    "equipment",
    "mash",
    "boil",
    "fermentation",
    "fermentable",
    "hop",
    "misc",
    "salt",
    "water",
    "yeast",
];

/// What the C++ holds in its `FolderInfo` struct: a folder's row id, its own name, and the path of
/// its parent.  Both strings are empty for a folder that sits at the root.
#[derive(Debug, Clone, Default)]
struct FolderInfo {
    id: i64,
    name: String,
    parent_path: String,
}

fn split_path(full_path: &str) -> FolderInfo {
    match full_path.rfind('/') {
        // "foo/bar/hum" splits into name "hum" and parent path "foo/bar".
        Some(last_slash) => FolderInfo {
            id: 0,
            name: full_path[last_slash + 1..].to_owned(),
            parent_path: full_path[..last_slash].to_owned(),
        },
        None => FolderInfo::default(),
    }
}

async fn execute(connection: &mut AnyConnection, sql: &str) -> Result<(), MigrationError> {
    sqlx::query(sql).execute(&mut *connection).await.map_err(|source| MigrationError::Statement {
        from: 19,
        to: 20,
        function: "migrate_to_20".to_owned(),
        sql: sql.to_owned(),
        source,
    })?;
    Ok(())
}

/// Phase one: create the folder tables and populate them with one row per distinct path string.
async fn create_folder_tables(connection: &mut AnyConnection, dialect: Dialect) -> Result<(), MigrationError> {
    let primary_key = render("{{primary_key}}", dialect)?;
    let text = render("{{type:string}}", dialect)?;
    let boolean = render("{{type:bool}}", dialect)?;
    let integer = render("{{type:int}}", dialect)?;

    for base_table in BASE_TABLES {
        let folder_table = format!("folder_for_{base_table}");
        let statements = [
            format!(
                "CREATE TABLE {folder_table} (id {primary_key}, name {text}, deleted {boolean}, \
                 contained_in_folder_id {integer}, \
                 FOREIGN KEY(contained_in_folder_id) REFERENCES {folder_table}(id) );"
            ),
            render(&format!("{{{{add_foreign_key:{base_table}|contained_in_folder_id|{folder_table}|id}}}}"), dialect)?,
            // Root folders are usually stored as "myFolder" rather than "/myFolder"; normalise the
            // stragglers so the path splitting below can assume it.
            format!("UPDATE {base_table} SET folder = SUBSTR(folder, 2) WHERE folder LIKE '/%'"),
            // One folder per distinct path, named with the whole path for now.  NULL is cast
            // because PostgreSQL will not infer the column type of a bare NULL here.
            format!(
                "INSERT INTO {folder_table} (name, deleted, contained_in_folder_id) \
                 SELECT DISTINCT folder, false AS deleted, CAST(NULL AS integer) AS contained_in_folder_id \
                 FROM {base_table} WHERE folder IS NOT NULL AND folder != '' ORDER BY folder"
            ),
            format!(
                "UPDATE {base_table} SET contained_in_folder_id = \
                 (SELECT id FROM {folder_table} WHERE {base_table}.folder = {folder_table}.name) \
                 WHERE folder IS NOT NULL AND folder != '' "
            ),
            format!("ALTER TABLE {base_table} DROP COLUMN folder"),
        ];
        for sql in statements {
            execute(&mut *connection, &sql).await?;
        }
    }
    Ok(())
}

/// Phase two: give every folder its own name and a reference to its parent.
///
/// The rows created above are named with their whole path, and a path may mention ancestors that
/// nothing referred to directly -- a row in `foo/bar/hum` implies folders `foo` and `foo/bar`.
/// Those are created here before the names are shortened.
async fn build_folder_tree(connection: &mut AnyConnection, dialect: Dialect) -> Result<(), MigrationError> {
    for base_table in BASE_TABLES {
        let folder_table = format!("folder_for_{base_table}");

        // A BTreeMap rather than the C++'s QHash: iteration order then decides the ids handed to
        // the folders created below, and a defined order makes a migration reproducible.
        let mut folders: BTreeMap<String, FolderInfo> = BTreeMap::new();
        let rows = sqlx::query(&format!("SELECT id, name AS fullPath FROM {folder_table} "))
            .fetch_all(&mut *connection)
            .await?;
        for row in rows {
            let full_path: String = row.try_get("fullPath")?;
            folders.insert(full_path.clone(), FolderInfo { id: row.try_get("id")?, ..split_path(&full_path) });
        }

        // Walk each path up towards the root, creating any ancestor that is missing.  Still named
        // with the full path, which keeps names unique until the ids are all known.
        let insert = bind_placeholders(
            &format!(
                "INSERT INTO {folder_table} (name, deleted, contained_in_folder_id) \
                 VALUES (?, false, CAST(NULL AS integer))"
            ),
            dialect,
        );
        let find = bind_placeholders(&format!("SELECT id FROM {folder_table} WHERE name = ?"), dialect);
        let mut created: BTreeMap<String, FolderInfo> = BTreeMap::new();
        for full_path in folders.keys().cloned().collect::<Vec<_>>() {
            let mut path = full_path;
            while let Some(last_slash) = path.rfind('/') {
                let parent_path = path[..last_slash].to_owned();
                if !folders.contains_key(&parent_path) && !created.contains_key(&parent_path) {
                    sqlx::query(&insert).bind(parent_path.clone()).execute(&mut *connection).await?;
                    let id: i64 = sqlx::query(&find)
                        .bind(parent_path.clone())
                        .fetch_one(&mut *connection)
                        .await?
                        .try_get("id")?;
                    created.insert(parent_path.clone(), FolderInfo { id, ..split_path(&parent_path) });
                }
                path = parent_path;
            }
        }
        folders.extend(created);

        // Now every folder exists, so each one can take its own name and point at its parent.  A
        // folder at the root keeps the path as its name and a NULL parent, so it is left alone.
        let update = bind_placeholders(
            &format!("UPDATE {folder_table} SET name = ?, contained_in_folder_id = ? WHERE id = ?"),
            dialect,
        );
        for info in folders.values() {
            if info.parent_path.is_empty() {
                continue;
            }
            let parent = folders.get(&info.parent_path).ok_or_else(|| MigrationError::Statement {
                from: 19,
                to: 20,
                function: "migrate_to_20".to_owned(),
                sql: format!("no parent folder {:?} for folder {:?} in {folder_table}", info.parent_path, info.name),
                source: sqlx::Error::RowNotFound,
            })?;
            sqlx::query(&update)
                .bind(info.name.clone())
                .bind(parent.id)
                .bind(info.id)
                .execute(&mut *connection)
                .await?;
        }
    }
    Ok(())
}

/// Schema v19 to v20.
pub async fn migrate_to_20(connection: &mut AnyConnection, dialect: Dialect) -> Result<(), MigrationError> {
    create_folder_tables(&mut *connection, dialect).await?;
    build_folder_tree(&mut *connection, dialect).await?;

    // Finally, a plain rename: "brew log" is the more widely used term for the record of a brew
    // day than "brew note".
    execute(&mut *connection, "ALTER TABLE brewnote RENAME TO brew_log").await?;
    for base_name in ["fermentable", "hop", "misc", "salt", "yeast"] {
        let sql = format!("ALTER TABLE {base_name}_stock_use  RENAME COLUMN brewnote_id to brew_log_id");
        execute(&mut *connection, &sql).await?;
    }
    Ok(())
}
