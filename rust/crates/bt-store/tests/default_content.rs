// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/tests/default_content.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Seeding a database with the content Brewtarget ships.
//!
//! Six files in three formats, which between them exercise every reader: BeerXML, BeerJSON and
//! `.beer`.  This is the strongest end-to-end test in the project -- real files, every format,
//! into a real database, loaded the way a fresh install would.

use bt_schema::Namespace;
use bt_store::default_content::{AVAILABLE_CONTENT_VERSION, Loaded, file_for, load, loaded_version};
use bt_store::{Store, dispatch, tables};

fn data_directory() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data")
}

async fn fresh(label: &str) -> Store {
    let path = std::env::temp_dir().join(format!("bt-content-{label}.sqlite"));
    let _ = std::fs::remove_file(&path);
    std::fs::File::create(&path).expect("create the database file");
    let store = Store::connect(&format!("sqlite://{}", path.display())).await.expect("connect");
    store.prepare(&Namespace::Default).await.expect("prepare");
    store
}

async fn count(store: &Store, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    dispatch!(store, |pool| { sqlx::query_scalar(&sql).fetch_one(pool).await.expect("count") })
}

/// Each step of the sequence has exactly one file; two would make the order ambiguous.
#[test]
fn every_version_in_the_sequence_has_one_file() {
    let directory = data_directory();
    for version in 1..=AVAILABLE_CONTENT_VERSION {
        let found = file_for(&directory, version).expect("read the directory");
        assert!(found.is_some(), "nothing for DefaultContent{version:03}");
    }
    assert_eq!(file_for(&directory, 99).expect("read"), None, "there is no ninety-ninth file");
}

#[tokio::test]
async fn a_fresh_database_gets_everything() {
    let store = fresh("fresh").await;
    assert_eq!(loaded_version(&store).await.expect("version"), 0, "a new database has none of it");

    let result = load(&store, &data_directory()).await.expect("load");
    let Loaded::UpTo { version, files, rows_written } = result else {
        panic!("expected content to be loaded, got {result:?}");
    };
    assert_eq!(version, AVAILABLE_CONTENT_VERSION);
    assert_eq!(files.len(), 6, "{files:?}");
    assert!(rows_written > 3000, "only wrote {rows_written} rows");

    // All three formats contributed: the hops come from BeerXML and BeerJSON, the recipes from
    // BeerXML and `.beer`, the styles from BeerJSON.
    assert!(count(&store, "hop").await > 700);
    assert!(count(&store, "fermentable").await > 400);
    assert!(count(&store, "yeast").await > 1200);
    assert!(count(&store, "style").await > 200);
    assert!(count(&store, "recipe").await > 30);
    assert!(count(&store, "mash").await > 40);

    assert_eq!(loaded_version(&store).await.expect("version"), AVAILABLE_CONTENT_VERSION);
}

/// The version is what makes this incremental: a release that adds a seventh file should load
/// only that one.  A database already at the current version has nothing to do.
#[tokio::test]
async fn a_database_already_up_to_date_is_left_alone() {
    let store = fresh("up-to-date").await;
    load(&store, &data_directory()).await.expect("first load");
    let hops = count(&store, "hop").await;

    let again = load(&store, &data_directory()).await.expect("second load");
    assert_eq!(again, Loaded::NothingToDo { version: AVAILABLE_CONTENT_VERSION });
    assert_eq!(count(&store, "hop").await, hops, "nothing should have been loaded twice");
}

/// A recipe from the content loads back as a whole recipe, which means every reference in the
/// files resolved to a real row.
#[tokio::test]
async fn a_recipe_from_the_content_comes_back_whole() {
    let store = fresh("recipe").await;
    load(&store, &data_directory()).await.expect("load");

    let recipes = tables::recipe::list(&store).await.expect("recipes");
    assert!(recipes.len() > 30);

    // Find one with hops in it and check the graph hangs together.
    let mut with_hops = 0;
    for row in &recipes {
        let recipe = bt_store::recipes::load(&store, row.id).await.expect("load").expect("the recipe");
        if !recipe.hops.is_empty() {
            with_hops += 1;
            assert!(
                recipe.hops.iter().all(|addition| addition.ingredient.is_some()),
                "{}: an addition lost its hop",
                recipe.name()
            );
        }
    }
    assert!(with_hops > 20, "only {with_hops} recipes had hops");
}
