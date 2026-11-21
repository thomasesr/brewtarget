// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/tests/migrations.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Run the migrations against the two real databases checked into this repository.
//!
//! `testData/Brewtarget_2.3.1_default_database.sqlite` records its version as the string "2.1.0",
//! which is schema version 3, and `data/default_db.sqlite` is at version 13.  Between them they
//! exercise the upgrade path from v3 to v23 end to end, which is the acceptance criterion for
//! PLAN.md Phase 2 -- and, usefully, needs no Qt.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use bt_schema::migrations::{manual, migrations, render};
use bt_schema::{Dialect, Namespace, migrate_to_latest, schema, schema_version};
use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Row};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().expect("repository root")
}

/// Copy a fixture somewhere writable; the originals are checked in and must stay untouched.
fn working_copy(fixture: &str, label: &str) -> PathBuf {
    let source = repository().join(fixture);
    let target = std::env::temp_dir().join(format!("bt-migration-{label}.sqlite"));
    let _ = std::fs::remove_file(&target);
    std::fs::copy(&source, &target).unwrap_or_else(|e| panic!("copy {}: {e}", source.display()));
    target
}

async fn open(path: &Path) -> AnyPool {
    sqlx::any::install_default_drivers();
    AnyPoolOptions::new()
        .max_connections(1)
        .connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("open working copy")
}

#[test]
fn every_migration_is_either_data_or_hand_ported() {
    manual::check_coverage().expect("hand-ported migrations do not match schema/migrations.json");
}

#[test]
fn every_statement_renders_for_every_dialect() {
    for dialect in Dialect::ALL {
        for migration in &migrations().migrations {
            for statement in migration.statements.iter().flatten() {
                render(&statement.sql, dialect)
                    .unwrap_or_else(|e| panic!("{} on {dialect}: {e}\n{}", migration.cpp_function, statement.sql));
            }
        }
    }
}

/// The tables and columns a migrated database ends up with must be the ones `schema/v23.json`
/// describes -- the same set the Phase 1 renderer would create from scratch.
async fn assert_matches_v23(pool: &AnyPool) {
    let created: BTreeSet<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
        .fetch_all(pool)
        .await
        .expect("list tables")
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .filter(|name| !name.starts_with("sqlite_"))
        .collect();
    // `settings` is not in schema/v23.json: the C++ creates it by hand rather than from the
    // ObjectStore definitions.  It still has to be there, since it holds the schema version.
    let mut expected: BTreeSet<String> = schema().tables.iter().map(|t| t.name.clone()).collect();
    expected.insert("settings".to_owned());
    assert_eq!(created, expected, "tables after migration");

    for table in &schema().tables {
        // Compared case-insensitively.  `recipe.carbonationTemp_c` has had that spelling since
        // before schema v3 and no migration renames it, while ObjectStoreTyped.cpp declares
        // `carbonationtemp_c`, so an upgraded database and a freshly created one disagree on its
        // case in the C++ too.  It makes no difference in practice: SQLite resolves column names
        // case-insensitively, and PostgreSQL folds unquoted identifiers to lower case.
        let actual: BTreeSet<String> = sqlx::query(&format!("SELECT name FROM pragma_table_info('{}')", table.name))
            .fetch_all(pool)
            .await
            .unwrap_or_else(|e| panic!("pragma_table_info({}): {e}", table.name))
            .into_iter()
            .map(|row| row.get::<String, _>("name").to_lowercase())
            .collect();
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        wanted.insert(table.primary_key.name.to_lowercase());
        wanted.extend(table.columns.iter().map(|c| c.name.to_lowercase()));
        wanted.extend(table.foreign_keys.iter().map(|f| f.column.to_lowercase()));
        assert_eq!(actual, wanted, "columns of {} after migration", table.name);
    }
    let _ = Namespace::Default;
}

#[tokio::test]
async fn migrates_the_2_3_1_database_from_v3() {
    let path = working_copy("testData/Brewtarget_2.3.1_default_database.sqlite", "v3");
    let pool = open(&path).await;
    let mut connection = pool.acquire().await.expect("connection");
    assert_eq!(schema_version(&mut connection).await.expect("read version"), 3);
    migrate_to_latest(&mut connection, Dialect::Sqlite).await.expect("migrate to latest");
    assert_eq!(schema_version(&mut connection).await.expect("read version"), 23);
    drop(connection);
    assert_matches_v23(&pool).await;
}

#[tokio::test]
async fn migrates_the_default_content_database_from_v13() {
    let path = working_copy("data/default_db.sqlite", "v13");
    let pool = open(&path).await;
    let mut connection = pool.acquire().await.expect("connection");
    assert_eq!(schema_version(&mut connection).await.expect("read version"), 13);
    migrate_to_latest(&mut connection, Dialect::Sqlite).await.expect("migrate to latest");
    assert_eq!(schema_version(&mut connection).await.expect("read version"), 23);
    drop(connection);
    assert_matches_v23(&pool).await;
}

async fn count(pool: &AnyPool, table: &str) -> i64 {
    sqlx::query(&format!("SELECT COUNT(*) AS n FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("count {table}: {e}"))
        .get("n")
}

#[tokio::test]
async fn migrating_preserves_the_data() {
    let path = working_copy("testData/Brewtarget_2.3.1_default_database.sqlite", "data");
    let pool = open(&path).await;

    // Tables that no migration between v3 and v23 adds to or removes from.
    let unchanged = ["recipe", "hop", "fermentable", "style", "yeast"];
    let mut before = Vec::new();
    for table in unchanged {
        before.push(count(&pool, table).await);
    }
    assert!(before.iter().all(|n| *n > 0), "fixture should not be empty: {before:?}");

    let mut connection = pool.acquire().await.expect("connection");
    migrate_to_latest(&mut connection, Dialect::Sqlite).await.expect("migrate to latest");
    drop(connection);

    for (table, expected) in unchanged.iter().zip(&before) {
        assert_eq!(count(&pool, table).await, *expected, "row count of {table}");
    }

    // Folders were path strings on each row at v19; by v23 they are rows of their own, referenced
    // by a foreign key.  Every recipe in this fixture sits in the "brewtarget" folder.
    let folders: Vec<String> = sqlx::query("SELECT name FROM folder_for_recipe")
        .fetch_all(&pool)
        .await
        .expect("folders")
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect();
    assert_eq!(folders, ["brewtarget"]);
    assert_eq!(count(&pool, "recipe WHERE contained_in_folder_id IS NOT NULL").await, before[0]);

    // migrate_to_15 seeds the nine standard water agents as salts, and migrate_to_21 moves them
    // into `misc`, tagging the names as it goes.  That is why `misc` is one of the tables whose
    // row count legitimately changes, and the fixture already had water agents of its own.
    assert_eq!(count(&pool, "misc WHERE name LIKE '%(migrated from salts)'").await, 9);
}

/// Collect every table's columns, lower-cased, so two databases can be compared.
async fn shape(pool: &AnyPool) -> BTreeSet<(String, String)> {
    let tables: Vec<String> = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
        .fetch_all(pool)
        .await
        .expect("list tables")
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .filter(|name| !name.starts_with("sqlite_"))
        .collect();

    let mut shape = BTreeSet::new();
    for table in tables {
        for row in sqlx::query(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .fetch_all(pool)
            .await
            .unwrap_or_else(|e| panic!("pragma_table_info({table}): {e}"))
        {
            shape.insert((table.to_lowercase(), row.get::<String, _>("name").to_lowercase()));
        }
    }
    shape
}

/// A database built from the schema definitions and one brought up by migration must match.
///
/// `DatabaseSchemaHelper::create` and the `migrate_to_*` chain are written independently of one
/// another in the C++, and are expected to agree; nothing there checks that they do.
#[tokio::test]
async fn creating_and_migrating_produce_the_same_database() {
    let fresh_path = std::env::temp_dir().join("bt-migration-fresh.sqlite");
    let _ = std::fs::remove_file(&fresh_path);
    std::fs::File::create(&fresh_path).expect("create file");
    let fresh = open(&fresh_path).await;
    let mut connection = fresh.acquire().await.expect("connection");
    bt_schema::create_database(&mut connection, Dialect::Sqlite, &Namespace::Default).await.expect("create");
    assert_eq!(schema_version(&mut connection).await.expect("read version"), 23);
    drop(connection);

    let migrated_path = working_copy("testData/Brewtarget_2.3.1_default_database.sqlite", "compare");
    let migrated = open(&migrated_path).await;
    let mut connection = migrated.acquire().await.expect("connection");
    migrate_to_latest(&mut connection, Dialect::Sqlite).await.expect("migrate");
    drop(connection);

    assert_eq!(shape(&fresh).await, shape(&migrated).await);
}
