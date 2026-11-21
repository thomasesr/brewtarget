// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/document.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading a whole `.beer` file into the graph of rows it describes.

use bt_format::document::{Document, read_dotbeer};
use bt_format::jsonc;
use bt_model::IngredientAmount;
use bt_model::tables::{HopInRecipeRow, MashStepRow, RecipeRow};

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read(subdirectory: &str, name: &str) -> Document {
    let path = repository().join("testData/dotBeer/examples").join(subdirectory).join(name);
    let document = jsonc::value_from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse");
    read_dotbeer(&document).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn a_whole_file_becomes_a_graph_of_records() {
    let document = read("recipes", "AmericanPaleAle.beer");
    assert_eq!(document.len(), 17, "sixteen standalone records and one recipe");

    for (collection, expected) in [
        ("fermentables", 3),
        ("hops", 3),
        ("miscs", 2),
        ("cultures", 1),
        ("waters", 2),
        ("styles", 1),
        ("equipments", 1),
        ("mashes", 1),
        ("boils", 1),
        ("fermentations", 1),
    ] {
        let found = document.collections.iter().find(|(name, _)| name == collection).map(|(_, records)| records.len());
        assert_eq!(found, Some(expected), "{collection}");
    }
    assert_eq!(document.recipes.len(), 1);
}

/// A recipe refers to the things it uses by local ID.  Those become foreign keys once the records
/// they name have been stored, which is the storing layer's job -- it is the only thing that
/// knows what key anything was given.
#[test]
fn a_recipe_refers_to_what_it_uses_by_local_id() {
    let document = read("recipes", "AmericanPaleAle.beer");
    let recipe = &document.recipes[0];

    for field in
        ["style_id", "equipment_id", "mash_id", "boil_id", "fermentation_id", "water_base_id", "water_target_id"]
    {
        let reference = recipe.record.references.get(field).unwrap_or_else(|| panic!("no {field}"));
        assert!(document.by_local_id(reference).is_some(), "{field} names {reference}, which is not in the file");
    }
    // The recipe itself has no local ID: nothing refers to a recipe.
    assert_eq!(recipe.record.local_id, None);
}

#[test]
fn a_recipe_owns_its_additions_instructions_and_brew_logs() {
    let document = read("recipes", "AmericanPaleAle.beer");
    let recipe = &document.recipes[0];
    let child = |field: &str| {
        recipe.children.iter().find(|(name, _)| name == field).unwrap_or_else(|| panic!("no {field}")).1.clone()
    };

    assert_eq!(child("ingredients/fermentable_additions").len(), 3);
    assert_eq!(child("ingredients/hop_additions").len(), 5);
    assert_eq!(child("ingredients/misc_additions").len(), 2);
    assert_eq!(child("ingredients/culture_additions").len(), 1);
    assert_eq!(child("instructions").len(), 3);
    assert_eq!(child("brew_logs").len(), 1);

    // Each addition names the ingredient it is an addition of.
    let hops = child("ingredients/hop_additions");
    assert_eq!(hops[0].record.table, "hop_in_recipe");
    assert_eq!(hops[0].record.references.get("hop_id").map(String::as_str), Some("hop_magnum"));
}

/// A mash, a boil and a fermentation each own their steps.
#[test]
fn step_owners_carry_their_steps() {
    let document = read("recipes", "AmericanPaleAle.beer");
    for (collection, field, table, expected) in [
        ("mashes", "mash_steps", "mash_step", 3),
        ("boils", "boil_steps", "boil_step", 4),
        ("fermentations", "fermentation_steps", "fermentation_step", 2),
    ] {
        let (_, records) = document.collections.iter().find(|(name, _)| name == collection).expect(collection);
        let steps = &records[0].children.iter().find(|(name, _)| name == field).expect(field).1;
        assert_eq!(steps.len(), expected, "{collection}");
        assert!(steps.iter().all(|step| step.record.table == table));
    }

    // And the steps deserialise with their values.
    let (_, mashes) = document.collections.iter().find(|(name, _)| name == "mashes").expect("mashes");
    let first: MashStepRow = mashes[0].children[0].1[0].record.clone().into_row().expect("deserialise a mash step");
    assert!(first.step_temp_c.is_some(), "a mash step should have a temperature");
}

/// An ingredient amount is one field in the file but two columns in the database, and is stored
/// as given rather than converted: 15 g of hops reads differently from 0.015 kg, and the unit the
/// recipe chose is part of what it says.
#[test]
fn an_ingredient_amount_becomes_a_quantity_and_a_unit() {
    let document = read("recipes", "AmericanPaleAle.beer");
    let (_, additions) = document.recipes[0]
        .children
        .iter()
        .find(|(name, _)| name == "ingredients/hop_additions")
        .expect("hop additions");

    let row: HopInRecipeRow = additions[0].record.clone().into_row().expect("deserialise");
    assert_eq!(row.quantity, Some(15.0));
    assert_eq!(row.unit.as_deref(), Some("grams"));

    // And it converts on demand, which is what the canonical amount is for.
    let canonical = row.canonical_amount().expect("an amount");
    assert_eq!(canonical.unit.name(), "kilograms");
    assert!((canonical.quantity - 0.015).abs() < 1e-12, "{}", canonical.quantity);
}

#[test]
fn a_recipe_deserialises_into_its_row() {
    let document = read("recipes", "AmericanPaleAle.beer");
    let row: RecipeRow = document.recipes[0].record.clone().into_row().expect("deserialise");
    assert_eq!(row.name.as_deref(), Some("Cascade Pale Ale"));
    assert!(row.batch_size.is_some());
    assert_eq!(row.id, 0, "no database identity until it is stored");
}

#[test]
fn every_valid_example_reads() {
    for (subdirectory, name) in [
        ("recipes", "AmericanPaleAle.beer"),
        ("recipes", "DrySaisonSubUnityFG.beer"),
        ("recipes", "GermanPilsColdCrash.beer"),
        ("stock", "StockPurchases.beer"),
        ("warnings", "DefaultBatchSizeTooBig.beer"),
    ] {
        let document = read(subdirectory, name);
        assert!(!document.is_empty(), "{name} should have records");
    }
}
