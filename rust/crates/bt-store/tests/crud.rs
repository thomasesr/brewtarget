// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/tests/crud.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Round-trip CRUD against a real database.
//!
//! Only SQLite runs here: PostgreSQL needs a server (see `bt-schema`'s gated test) and MariaDB is
//! not available in this environment at all.  The repositories are generated from one template,
//! so what differs between backends is the placeholder style and the pool type, both of which the
//! dispatch macro settles at compile time.

use bt_model::enums::HopForm;
use bt_model::tables::{HopRow, RecipeRow};
use bt_schema::Namespace;
use bt_store::{Store, tables};

async fn fresh_store(label: &str) -> Store {
    let path = std::env::temp_dir().join(format!("bt-store-{label}.sqlite"));
    let _ = std::fs::remove_file(&path);
    std::fs::File::create(&path).expect("create database file");
    let store = Store::connect(&format!("sqlite://{}", path.display())).await.expect("connect");
    let version = store.prepare(&Namespace::Default).await.expect("prepare");
    assert_eq!(version, 23);
    store
}

fn a_hop() -> HopRow {
    HopRow {
        name: Some("Saaz".to_owned()),
        deleted: Some(false),
        alpha_pct: Some(3.5),
        beta_pct: Some(4.0),
        origin: Some("Czech Republic".to_owned()),
        form: Some(HopForm::Leaf.as_str().to_owned()),
        notes: Some("Noble".to_owned()),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_row_survives_a_round_trip() {
    let store = fresh_store("roundtrip").await;

    let id = tables::hop::insert(&store, &a_hop()).await.expect("insert");
    assert!(id > 0, "the database should have generated a key");

    let read = tables::hop::get(&store, id).await.expect("get").expect("the row we just inserted");
    assert_eq!(read.id, id);
    assert_eq!(read.name.as_deref(), Some("Saaz"));
    assert_eq!(read.alpha_pct, Some(3.5));
    assert_eq!(read.origin.as_deref(), Some("Czech Republic"));
    // The form column stores the string, which parses back to the enum.
    assert_eq!(read.form.as_deref().map(str::parse::<HopForm>).transpose().expect("valid form"), Some(HopForm::Leaf));

    // Everything but the key should match what went in.
    let expected = HopRow { id, ..a_hop() };
    assert_eq!(read, expected);
}

#[tokio::test]
async fn an_update_replaces_every_column() {
    let store = fresh_store("update").await;
    let id = tables::hop::insert(&store, &a_hop()).await.expect("insert");

    let mut row = tables::hop::get(&store, id).await.expect("get").expect("row");
    row.alpha_pct = Some(4.25);
    row.notes = None;
    row.form = Some(HopForm::Pellet.as_str().to_owned());
    tables::hop::update(&store, &row).await.expect("update");

    let read = tables::hop::get(&store, id).await.expect("get").expect("row");
    assert_eq!(read.alpha_pct, Some(4.25));
    assert_eq!(read.notes, None, "a column set back to NULL should be cleared, not left alone");
    assert_eq!(read.form.as_deref(), Some("pellet"));
    assert_eq!(read, row);
}

#[tokio::test]
async fn deleting_is_soft_by_default() {
    let store = fresh_store("delete").await;
    let id = tables::hop::insert(&store, &a_hop()).await.expect("insert");

    tables::hop::soft_delete(&store, id, true).await.expect("soft delete");
    let read = tables::hop::get(&store, id).await.expect("get").expect("still there");
    assert_eq!(read.deleted, Some(true), "a soft-deleted row is kept so recipes using it still resolve");

    tables::hop::delete(&store, id).await.expect("hard delete");
    assert!(tables::hop::get(&store, id).await.expect("get").is_none());
}

#[tokio::test]
async fn listing_returns_everything_inserted() {
    let store = fresh_store("list").await;
    assert!(tables::hop::list(&store).await.expect("list").is_empty());

    for name in ["Saaz", "Fuggle", "Cascade"] {
        let row = HopRow { name: Some(name.to_owned()), ..a_hop() };
        tables::hop::insert(&store, &row).await.expect("insert");
    }
    let mut names: Vec<String> =
        tables::hop::list(&store).await.expect("list").into_iter().filter_map(|r| r.name).collect();
    names.sort();
    assert_eq!(names, ["Cascade", "Fuggle", "Saaz"]);
}

#[tokio::test]
async fn foreign_keys_are_stored_and_read_back() {
    let store = fresh_store("fk").await;

    let folder = tables::folder_for_recipe::insert(
        &store,
        &bt_model::tables::FolderForRecipeRow {
            name: Some("brewtarget".to_owned()),
            deleted: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("insert folder");

    let recipe = RecipeRow {
        name: Some("Best Bitter".to_owned()),
        deleted: Some(false),
        contained_in_folder_id: Some(folder),
        ..Default::default()
    };
    let id = tables::recipe::insert(&store, &recipe).await.expect("insert recipe");
    let read = tables::recipe::get(&store, id).await.expect("get").expect("row");
    assert_eq!(read.contained_in_folder_id, Some(folder));
}

#[tokio::test]
async fn folders_are_created_along_a_path_and_read_back_as_a_tree() {
    let store = fresh_store("folders").await;

    // Nothing exists yet, and the root is no folder at all rather than a folder named "".
    assert_eq!(bt_store::folders::ensure_path(&store, "recipe", "").await.expect("root"), None);
    assert!(bt_store::folders::load_tree(&store, "recipe").await.expect("tree").is_empty());

    let hum = bt_store::folders::ensure_path(&store, "recipe", "foo/bar/hum").await.expect("ensure").expect("an id");
    let tree = bt_store::folders::load_tree(&store, "recipe").await.expect("tree");
    assert_eq!(tree.len(), 3, "the whole chain should have been created");
    assert_eq!(tree.full_path(hum).as_deref(), Some("foo/bar/hum"));

    // Asking again finds what is there rather than creating a second copy.
    let again = bt_store::folders::ensure_path(&store, "recipe", "/foo/bar/hum/").await.expect("ensure").expect("id");
    assert_eq!(again, hum);
    assert_eq!(bt_store::folders::load_tree(&store, "recipe").await.expect("tree").len(), 3);

    // A sibling branch reuses the part of the path that already exists.
    bt_store::folders::ensure_path(&store, "recipe", "foo/other").await.expect("ensure");
    let tree = bt_store::folders::load_tree(&store, "recipe").await.expect("tree");
    assert_eq!(tree.len(), 4);
    assert_eq!(tree.children(None).len(), 1, "still one root");

    assert_eq!(
        bt_store::folders::full_path(&store, "recipe", hum).await.expect("path").as_deref(),
        Some("foo/bar/hum")
    );
}

#[tokio::test]
async fn each_kind_of_thing_keeps_its_own_folders() {
    let store = fresh_store("folder-kinds").await;

    bt_store::folders::ensure_path(&store, "recipe", "shared-name").await.expect("recipe folder");
    assert!(bt_store::folders::load_tree(&store, "hop").await.expect("tree").is_empty(), "hops have their own table");

    // And a kind with no folder table is rejected rather than reaching SQL as a table name.
    let error = bt_store::folders::load_tree(&store, "nonsense").await.expect_err("no such folder table");
    assert!(error.to_string().contains("folder_for_nonsense"), "{error}");
}
