// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/round_trip.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading a record, writing it back out, and reading it again.
//!
//! The property that matters is that the second read gives the same columns as the first.  A
//! file is not expected to come back byte-identical -- it had comments and its own choice of
//! units -- but nothing the database stores may be lost or changed on the way through.

use bt_format::jsonc;
use bt_format::mapping::{map_collection, map_record};
use bt_format::writing::write_record;

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn example(name: &str) -> serde_json::Value {
    let path = repository().join("testData/dotBeer/examples/recipes").join(name);
    jsonc::value_from_str(&std::fs::read_to_string(&path).expect("read")).expect("parse")
}

/// Read a collection, write each record back out, read it again, and compare.
fn round_trip(document: &serde_json::Value, collection: &str) -> usize {
    let first = map_collection("dotBeer", document, "DotBeer", collection).expect(collection);
    let checked = first.len();

    for record in first {
        let written = write_record(
            "dotBeer",
            collection,
            record.table,
            &record.columns,
            record.local_id.as_deref(),
            record.folder_path.as_deref(),
        )
        .expect("write");

        let again = map_record("dotBeer", collection, &written).expect("read back");
        assert_eq!(again.table, record.table);
        assert_eq!(again.local_id, record.local_id, "{collection}: local ID lost");
        assert_eq!(again.folder_path, record.folder_path, "{collection}: folder path lost");
        assert_eq!(again.columns, record.columns, "{collection}: columns differ after a round trip\nwrote: {written}");
    }
    checked
}

#[test]
fn ingredients_survive_a_round_trip() {
    let document = example("AmericanPaleAle.beer");
    for collection in ["hops", "fermentables", "cultures", "miscs", "waters", "styles"] {
        assert!(round_trip(&document, collection) > 0, "{collection} should have records to check");
    }
}

#[test]
fn equipment_and_profiles_survive_a_round_trip() {
    let document = example("AmericanPaleAle.beer");
    for collection in ["equipments", "mashes", "boils", "fermentations"] {
        assert!(round_trip(&document, collection) > 0, "{collection} should have records to check");
    }
}

#[test]
fn every_example_recipe_survives_a_round_trip() {
    let mut checked = 0;
    for name in ["AmericanPaleAle.beer", "DrySaisonSubUnityFG.beer", "GermanPilsColdCrash.beer"] {
        let document = example(name);
        // Not every recipe uses every collection, so an empty one is fine here.
        for collection in [
            "hops",
            "fermentables",
            "cultures",
            "miscs",
            "waters",
            "styles",
            "equipments",
            "mashes",
            "boils",
            "fermentations",
        ] {
            checked += round_trip(&document, collection);
        }
    }
    assert!(checked >= 30, "only round-tripped {checked} records");
}

/// A measurement comes back out in the canonical unit for what it measures, which is what the
/// column holds.  The file may have used something else -- the fixture gives hop alpha ranges as
/// percentages and water volumes in litres -- and the value is the same either way.
#[test]
fn measurements_are_written_in_canonical_units() {
    let document = example("AmericanPaleAle.beer");
    let equipments = map_collection("dotBeer", &document, "DotBeer", "equipments").expect("equipments");
    let written = write_record(
        "dotBeer",
        "equipments",
        equipments[0].table,
        &equipments[0].columns,
        equipments[0].local_id.as_deref(),
        equipments[0].folder_path.as_deref(),
    )
    .expect("write");

    // Volumes are litres, not the barrels the C++ would reach for.
    let volumes: Vec<&str> = find_units(&written, &mut Vec::new());
    assert!(volumes.contains(&"l"), "expected litres among {volumes:?}");
    assert!(!volumes.contains(&"bbl"), "a 25 litre cooler should not be written in barrels");
}

/// Collect every `unit` a written record mentions.
fn find_units<'a>(node: &'a serde_json::Value, found: &mut Vec<&'a str>) -> Vec<&'a str> {
    match node {
        serde_json::Value::Object(object) => {
            if let Some(unit) = object.get("unit").and_then(serde_json::Value::as_str) {
                found.push(unit);
            }
            for value in object.values() {
                find_units(value, found);
            }
        }
        serde_json::Value::Array(array) => {
            for value in array {
                find_units(value, found);
            }
        }
        _ => {}
    }
    found.clone()
}
