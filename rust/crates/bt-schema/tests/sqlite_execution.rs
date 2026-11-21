// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/tests/sqlite_execution.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Execute the generated SQLite DDL for real and check the resulting database.
//!
//! Byte-comparison against the C++ output is still pending a dump from a Qt6-capable machine, but
//! executing the DDL catches anything syntactically wrong, and comparing `pragma_table_info`
//! against `schema/v23.json` catches a column that was dropped, duplicated or mistyped on the way
//! through the renderer. See PLAN.md Phase 1.

use std::collections::BTreeSet;

use bt_schema::{Dialect, Namespace, create_all, schema};
use sqlx::Row;
use sqlx::sqlite::SqlitePoolOptions;

#[tokio::test]
async fn generated_sqlite_ddl_builds_the_expected_database() {
    // max_connections(1) keeps every statement on the one in-memory database.
    let pool =
        SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.expect("open in-memory SQLite");

    for statement in create_all(Dialect::Sqlite, &Namespace::Default).unwrap() {
        sqlx::query(&statement).execute(&pool).await.unwrap_or_else(|e| panic!("failed to execute:\n{statement}\n{e}"));
    }

    let created: BTreeSet<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
        .fetch_all(&pool)
        .await
        .expect("list tables")
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    let mut expected: BTreeSet<String> = schema().tables.iter().map(|t| t.name.clone()).collect();
    expected.insert("settings".to_owned()); // created by hand, see bt_schema::settings_table
    assert_eq!(created, expected);

    for table in &schema().tables {
        let actual: BTreeSet<String> = sqlx::query(&format!("SELECT name FROM pragma_table_info('{}')", table.name))
            .fetch_all(&pool)
            .await
            .unwrap_or_else(|e| panic!("pragma_table_info({}): {e}", table.name))
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect();

        // Every table's columns are its primary key, its ordinary columns, and the foreign keys
        // that were bolted on afterwards by ALTER TABLE.
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        wanted.insert(table.primary_key.name.clone());
        wanted.extend(table.columns.iter().map(|c| c.name.clone()));
        wanted.extend(table.foreign_keys.iter().map(|f| f.column.clone()));

        assert_eq!(actual, wanted, "columns of {}", table.name);
    }
}

#[tokio::test]
async fn foreign_keys_actually_reference_their_targets() {
    let pool =
        SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.expect("open in-memory SQLite");
    for statement in create_all(Dialect::Sqlite, &Namespace::Default).unwrap() {
        sqlx::query(&statement).execute(&pool).await.expect("execute DDL");
    }

    for table in &schema().tables {
        let rows =
            sqlx::query(&format!("SELECT \"table\", \"from\", \"to\" FROM pragma_foreign_key_list('{}')", table.name))
                .fetch_all(&pool)
                .await
                .unwrap_or_else(|e| panic!("pragma_foreign_key_list({}): {e}", table.name));
        let actual: BTreeSet<(String, String, String)> = rows
            .into_iter()
            .map(|r| (r.get::<String, _>("from"), r.get::<String, _>("table"), r.get::<String, _>("to")))
            .collect();
        let wanted: BTreeSet<(String, String, String)> = table
            .foreign_keys
            .iter()
            .map(|f| (f.column.clone(), f.references_table.clone(), f.references_column.clone()))
            .collect();
        assert_eq!(actual, wanted, "foreign keys of {}", table.name);
    }
}
