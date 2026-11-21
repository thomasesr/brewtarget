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

/// Build a small but complete recipe and read the whole graph back.
#[tokio::test]
async fn a_recipe_is_reassembled_from_the_tables_it_is_spread_across() {
    use bt_model::IngredientAmount;
    use bt_model::enums::RecipeAdditionStage;
    use bt_model::tables::*;

    let store = fresh_store("recipe-aggregate").await;

    let hop_id = tables::hop::insert(&store, &a_hop()).await.expect("hop");
    let fermentable_id = tables::fermentable::insert(
        &store,
        &FermentableRow { name: Some("Maris Otter".to_owned()), deleted: Some(false), ..Default::default() },
    )
    .await
    .expect("fermentable");
    let style_id = tables::style::insert(
        &store,
        &StyleRow { name: Some("Best Bitter".to_owned()), deleted: Some(false), ..Default::default() },
    )
    .await
    .expect("style");
    let mash_id = tables::mash::insert(
        &store,
        &MashRow { name: Some("Single infusion".to_owned()), deleted: Some(false), ..Default::default() },
    )
    .await
    .expect("mash");

    let recipe_id = tables::recipe::insert(
        &store,
        &RecipeRow {
            name: Some("Ordinary Bitter".to_owned()),
            deleted: Some(false),
            style_id: Some(style_id),
            mash_id: Some(mash_id),
            batch_size: Some(19.87),
            ..Default::default()
        },
    )
    .await
    .expect("recipe");

    // Steps come back in step_number order, so insert them out of order to prove it.
    for (number, temp) in [(2, 72.0), (1, 66.0)] {
        tables::mash_step::insert(
            &store,
            &MashStepRow {
                name: Some(format!("Step {number}")),
                deleted: Some(false),
                mash_id: Some(mash_id),
                step_number: Some(number),
                step_temp_c: Some(temp),
                ..Default::default()
            },
        )
        .await
        .expect("mash step");
    }

    let mut hop_addition = HopInRecipeRow {
        name: Some("Fuggle at 60".to_owned()),
        deleted: Some(false),
        recipe_id: Some(recipe_id),
        hop_id: Some(hop_id),
        quantity: Some(28.0),
        duration_mins: Some(60.0),
        ..Default::default()
    };
    hop_addition.set_unit(bt_measure::Unit::by_name("grams"));
    hop_addition.set_stage(Some(RecipeAdditionStage::Boil));
    tables::hop_in_recipe::insert(&store, &hop_addition).await.expect("hop addition");

    let mut fermentable_addition = FermentableInRecipeRow {
        name: Some("Maris Otter".to_owned()),
        deleted: Some(false),
        recipe_id: Some(recipe_id),
        fermentable_id: Some(fermentable_id),
        quantity: Some(4.5),
        ..Default::default()
    };
    fermentable_addition.set_unit(bt_measure::Unit::by_name("kilograms"));
    fermentable_addition.set_stage(Some(RecipeAdditionStage::Mash));
    tables::fermentable_in_recipe::insert(&store, &fermentable_addition).await.expect("fermentable addition");

    let recipe = bt_store::recipes::load(&store, recipe_id).await.expect("load").expect("the recipe");

    assert_eq!(recipe.name(), "Ordinary Bitter");
    assert_eq!(recipe.style.as_ref().and_then(|s| s.name.as_deref()), Some("Best Bitter"));
    assert!(recipe.equipment.is_none(), "this recipe has no equipment");
    assert_eq!(recipe.instructions.len(), 0);

    // Additions carry the ingredient they refer to rather than a copy of it.
    assert_eq!(recipe.hops.len(), 1);
    assert_eq!(recipe.hops[0].ingredient.as_ref().and_then(|h| h.name.as_deref()), Some("Saaz"));
    assert_eq!(recipe.fermentables[0].ingredient.as_ref().and_then(|f| f.name.as_deref()), Some("Maris Otter"));

    // 28 g of hops is 0.028 kg once converted to the canonical unit for a mass.
    let amount = recipe.hops[0].addition.canonical_amount().expect("an amount");
    assert_eq!(amount.unit.name(), "kilograms");
    assert!((amount.quantity - 0.028).abs() < 1e-9, "{}", amount.quantity);

    // Stage filtering looks through the stored strings.
    assert_eq!(recipe.hops_at(RecipeAdditionStage::Boil).count(), 1);
    assert_eq!(recipe.hops_at(RecipeAdditionStage::Mash).count(), 0);
    assert_eq!(recipe.fermentables_at(RecipeAdditionStage::Mash).count(), 1);

    // Steps come back ordered, whatever order they went in.
    let temperatures: Vec<f64> = recipe.mash_steps().iter().filter_map(|step| step.step_temp_c).collect();
    assert_eq!(temperatures, [66.0, 72.0]);

    assert!(bt_store::recipes::load(&store, 9999).await.expect("load").is_none());
}

/// Deleting a recipe takes what it owns with it, but leaves profiles alone.
#[tokio::test]
async fn deleting_a_recipe_removes_what_it_owns_but_keeps_named_profiles() {
    use bt_model::tables::*;

    let store = fresh_store("recipe-delete").await;

    // A named mash is a profile the user meant to reuse.
    let named_mash =
        tables::mash::insert(&store, &MashRow { name: Some("Single infusion".to_owned()), ..Default::default() })
            .await
            .expect("mash");
    // An unnamed boil was made for this recipe alone.
    let unnamed_boil =
        tables::boil::insert(&store, &BoilRow { name: Some(String::new()), ..Default::default() }).await.expect("boil");
    tables::boil_step::insert(
        &store,
        &BoilStepRow { boil_id: Some(unnamed_boil), step_number: Some(1), ..Default::default() },
    )
    .await
    .expect("boil step");

    let recipe_id = tables::recipe::insert(
        &store,
        &RecipeRow {
            name: Some("Doomed".to_owned()),
            mash_id: Some(named_mash),
            boil_id: Some(unnamed_boil),
            ..Default::default()
        },
    )
    .await
    .expect("recipe");

    tables::hop_in_recipe::insert(
        &store,
        &HopInRecipeRow { recipe_id: Some(recipe_id), quantity: Some(28.0), ..Default::default() },
    )
    .await
    .expect("hop addition");
    tables::instruction::insert(
        &store,
        &InstructionRow { recipe_id: Some(recipe_id), step_number: Some(1), ..Default::default() },
    )
    .await
    .expect("instruction");

    bt_store::recipes::delete(&store, recipe_id).await.expect("delete");

    assert!(tables::recipe::get(&store, recipe_id).await.expect("get").is_none());
    assert!(tables::hop_in_recipe::list(&store).await.expect("list").is_empty(), "additions go with the recipe");
    assert!(tables::instruction::list(&store).await.expect("list").is_empty(), "instructions go with the recipe");

    assert!(tables::mash::get(&store, named_mash).await.expect("get").is_some(), "a named profile is kept");
    assert!(tables::boil::get(&store, unnamed_boil).await.expect("get").is_none(), "an unnamed one-off is cleaned up");
    assert!(tables::boil_step::list(&store).await.expect("list").is_empty(), "and takes its steps with it");

    // Deleting something that is not there is not an error.
    bt_store::recipes::delete(&store, recipe_id).await.expect("second delete");
}

/// An unnamed profile shared by two recipes survives the first of them being deleted.
#[tokio::test]
async fn an_unnamed_profile_still_in_use_is_left_alone() {
    use bt_model::tables::*;

    let store = fresh_store("recipe-delete-shared").await;
    let shared_boil =
        tables::boil::insert(&store, &BoilRow { name: Some(String::new()), ..Default::default() }).await.expect("boil");

    let mut ids = Vec::new();
    for name in ["First", "Second"] {
        ids.push(
            tables::recipe::insert(
                &store,
                &RecipeRow { name: Some(name.to_owned()), boil_id: Some(shared_boil), ..Default::default() },
            )
            .await
            .expect("recipe"),
        );
    }

    bt_store::recipes::delete(&store, ids[0]).await.expect("delete the first");
    assert!(tables::boil::get(&store, shared_boil).await.expect("get").is_some(), "the second recipe still uses it");

    bt_store::recipes::delete(&store, ids[1]).await.expect("delete the second");
    assert!(tables::boil::get(&store, shared_boil).await.expect("get").is_none(), "now nothing uses it");
}

/// Import a whole `.beer` file and check the graph came out the other side.
#[tokio::test]
async fn a_dotbeer_file_is_stored_with_its_references_resolved() {
    let store = fresh_store("import").await;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../testData/dotBeer/examples/recipes/AmericanPaleAle.beer");
    let source = std::fs::read_to_string(&path).expect("read the fixture");
    let parsed = bt_format::jsonc::value_from_str(&source).expect("parse");
    let document = bt_format::document::read_dotbeer(&parsed).expect("read");

    let imported = bt_store::import::import(&store, &document).await.expect("import");

    // Seventeen records plus the rows they own: steps, additions, instructions, a brew log.
    assert!(imported.rows_written > 17, "only wrote {}", imported.rows_written);
    assert_eq!(imported.recipe_ids.len(), 1);

    // Every local ID in the file now has a key.
    assert!(imported.by_local_id.contains_key("hop_magnum"));
    assert!(imported.by_local_id.contains_key("style_bjcp2021_18b"));
    assert_eq!(imported.by_local_id["hop_magnum"].0, "hop");

    // The ingredients are all there.
    assert_eq!(tables::hop::list(&store).await.expect("hops").len(), 3);
    assert_eq!(tables::fermentable::list(&store).await.expect("fermentables").len(), 3);
    assert_eq!(tables::yeast::list(&store).await.expect("cultures").len(), 1);
    assert_eq!(tables::style::list(&store).await.expect("styles").len(), 1);

    // And the recipe points at them by key rather than by local ID.
    let recipe_id = imported.recipe_ids[0];
    let recipe = bt_store::recipes::load(&store, recipe_id).await.expect("load").expect("the recipe");
    assert_eq!(recipe.name(), "Cascade Pale Ale");
    assert_eq!(recipe.style.as_ref().and_then(|style| style.name.as_deref()), Some("American Pale Ale"));
    assert!(recipe.equipment.is_some(), "the equipment reference should have resolved");

    // The additions came across with their ingredients and amounts.
    assert_eq!(recipe.hops.len(), 5);
    assert_eq!(recipe.fermentables.len(), 3);
    assert_eq!(recipe.yeasts.len(), 1);
    let magnum = recipe
        .hops
        .iter()
        .find(|addition| addition.ingredient.as_ref().and_then(|hop| hop.name.as_deref()) == Some("Magnum"));
    let magnum = magnum.expect("the Magnum addition");
    assert_eq!(magnum.addition.quantity, Some(15.0));
    assert_eq!(magnum.addition.unit.as_deref(), Some("grams"));

    // The mash and its steps came too, in order.
    let temperatures: Vec<f64> = recipe.mash_steps().iter().filter_map(|step| step.step_temp_c).collect();
    assert_eq!(temperatures.len(), 3, "three mash steps");
    assert!(temperatures.iter().all(|temperature| *temperature > 0.0));

    // Folder paths became folder rows.
    let hop_folders = bt_store::folders::load_tree(&store, "hop").await.expect("hop folders");
    assert!(!hop_folders.is_empty(), "the hops are filed somewhere");
    let hop = tables::hop::list(&store).await.expect("hops").into_iter().find(|h| h.name.as_deref() == Some("Magnum"));
    let hop = hop.expect("Magnum");
    let folder_id = hop.contained_in_folder_id.expect("Magnum is in a folder");
    assert_eq!(hop_folders.full_path(folder_id).as_deref(), Some("Hops/German – Noble"));
}

/// Importing the same file twice gives two independent copies rather than failing or merging.
#[tokio::test]
async fn importing_twice_does_not_collide() {
    let store = fresh_store("import-twice").await;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../testData/dotBeer/examples/recipes/GermanPilsColdCrash.beer");
    let source = std::fs::read_to_string(&path).expect("read");
    let parsed = bt_format::jsonc::value_from_str(&source).expect("parse");
    let document = bt_format::document::read_dotbeer(&parsed).expect("read");

    let first = bt_store::import::import(&store, &document).await.expect("first import");
    let second = bt_store::import::import(&store, &document).await.expect("second import");

    assert_ne!(first.recipe_ids, second.recipe_ids, "the second import gets its own keys");
    assert_eq!(tables::recipe::list(&store).await.expect("recipes").len(), 2);
    // Local IDs are per file, so the second import reuses the names with different keys.
    assert_eq!(first.by_local_id.len(), second.by_local_id.len());
}
