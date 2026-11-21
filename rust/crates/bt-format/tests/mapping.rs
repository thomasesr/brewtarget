// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/mapping.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading `.beer` records into database rows.
//!
//! The kitchen-sink American Pale Ale fixture is the one to test against: it deliberately uses
//! every top-level section of the format, so a field that is read wrongly shows up here.

use bt_format::jsonc;
use bt_format::mapping::{map_collection, map_record};
use bt_model::enums::{HopForm, HopType};
use bt_model::tables::{EquipmentRow, FermentableRow, HopRow, StyleRow, WaterRow, YeastRow};

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn american_pale_ale() -> serde_json::Value {
    let path = repository().join("testData/dotBeer/examples/recipes/AmericanPaleAle.beer");
    jsonc::value_from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse")
}

#[test]
fn ingredients_become_rows_of_the_right_table() {
    let document = american_pale_ale();
    for (collection, table, expected) in [
        ("hops", "hop", 3),
        ("fermentables", "fermentable", 3),
        ("cultures", "yeast", 1),
        ("miscs", "misc", 2),
        ("waters", "water", 2),
        ("styles", "style", 1),
        ("equipments", "equipment", 1),
    ] {
        let rows = map_collection("dotBeer", &document, "DotBeer", collection).expect(collection);
        assert_eq!(rows.len(), expected, "{collection}");
        assert!(rows.iter().all(|row| row.table == table), "{collection} should map to {table}");
        assert!(rows.iter().all(|row| !row.columns.is_empty()), "{collection} rows should have columns");
    }
}

/// A hop read out of the file should deserialise into the generated row struct and come back
/// with the values the file gives.
#[test]
fn a_hop_round_trips_into_its_row() {
    let rows = map_collection("dotBeer", &american_pale_ale(), "DotBeer", "hops").expect("hops");
    let magnum = rows.into_iter().find(|row| row.local_id.as_deref() == Some("hop_magnum")).expect("Magnum");

    assert_eq!(magnum.folder_path.as_deref(), Some("/Hops/German – Noble"));
    let row: HopRow = magnum.into_row().expect("deserialise");

    assert_eq!(row.id, 0, "a row read from a file has no database identity yet");
    assert_eq!(row.name.as_deref(), Some("Magnum"));
    assert_eq!(row.origin.as_deref(), Some("Germany"));
    assert_eq!(row.alpha_pct, Some(13.5));
    assert_eq!(row.alpha_min_pct, Some(12.0));
    assert_eq!(row.alpha_max_pct, Some(16.0));
    assert_eq!(row.year.as_deref(), Some("2025"));

    // Enum columns hold the string the database stores, which parses back to the typed form.
    assert_eq!(row.form().expect("valid form"), Some(HopForm::Pellet));
    assert_eq!(row.htype().expect("valid type"), Some(HopType::Bittering));
}

/// Fields the file does not mention stay absent rather than becoming zero.
#[test]
fn missing_fields_are_left_unset() {
    let rows = map_collection("dotBeer", &american_pale_ale(), "DotBeer", "hops").expect("hops");
    let centennial =
        rows.into_iter().find(|row| row.local_id.as_deref() == Some("hop_centennial")).expect("Centennial");
    let row: HopRow = centennial.into_row().expect("deserialise");
    assert_eq!(row.alpha_pct, Some(10.0));
    assert_eq!(row.year, None, "this hop has no year, which is not the same as a year of zero");
}

#[test]
fn every_kind_of_ingredient_deserialises() {
    let document = american_pale_ale();
    let first = |collection: &str| {
        map_collection("dotBeer", &document, "DotBeer", collection)
            .unwrap_or_else(|e| panic!("{collection}: {e}"))
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{collection} is empty"))
    };

    let fermentable: FermentableRow = first("fermentables").into_row().expect("fermentable");
    assert_eq!(fermentable.name.as_deref(), Some("Pale Ale Malt"));
    assert_eq!(fermentable.potential_yield_sg, Some(1.037));

    let yeast: YeastRow = first("cultures").into_row().expect("culture");
    assert_eq!(yeast.name.as_deref(), Some("SafAle US-05"));
    assert_eq!(yeast.attenuation_min_pct, Some(78.0));

    let water: WaterRow = first("waters").into_row().expect("water");
    assert_eq!(water.calcium, Some(75.0));

    let style: StyleRow = first("styles").into_row().expect("style");
    assert_eq!(style.name.as_deref(), Some("American Pale Ale"));
    assert_eq!(style.category_number.as_deref(), Some("18"));

    let equipment: EquipmentRow = first("equipments").into_row().expect("equipment");
    assert_eq!(equipment.name.as_deref(), Some("25 L All-Grain Cooler System"));
}

/// A local ID and a folder path are not columns: the first exists only inside the file, and the
/// second has to become a folder row before anything can point at it.
#[test]
fn local_ids_and_folder_paths_are_kept_apart_from_the_columns() {
    let rows = map_collection("dotBeer", &american_pale_ale(), "DotBeer", "fermentables").expect("fermentables");
    let first = rows.first().expect("a fermentable");
    assert!(first.local_id.is_some());
    assert!(first.folder_path.is_some());
    assert!(!first.columns.contains_key("local_id"), "local_id is not stored");
    assert!(!first.columns.contains_key("folder_path"), "a folder path becomes a folder row");
}

#[test]
fn an_unknown_format_or_record_is_an_error() {
    let document = american_pale_ale();
    assert!(map_collection("beerSmith", &document, "DotBeer", "hops").is_err());
    assert!(map_collection("dotBeer", &document, "DotBeer", "unicorns").is_err());
    assert!(map_record("dotBeer", "unicorns", &serde_json::json!({})).is_err());
}

/// A collection the file does not have is empty rather than an error: these formats make nearly
/// everything optional.
#[test]
fn an_absent_collection_is_simply_empty() {
    let rows = map_collection("dotBeer", &serde_json::json!({"DotBeer": {}}), "DotBeer", "hops").expect("hops");
    assert!(rows.is_empty());
}
