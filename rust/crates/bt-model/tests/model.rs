// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-model/tests/model.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Checks on the generated model code.
//!
//! The strings these enumerations map to are on-disk data: a database written with the wrong one
//! will not read back in the C++ application.  So the generated code is checked against
//! `schema/enums.json` rather than being trusted because it was generated.

use std::str::FromStr;

use bt_model::enums::{FermentableType, HopForm, RecipeAdditionStage, YeastForm};
use bt_model::tables::{HopRow, RecipeRow, TABLES};

#[test]
fn every_table_has_a_row_struct() {
    assert_eq!(TABLES.len(), 39);
    assert!(TABLES.contains(&"hop"));
    assert!(TABLES.contains(&"folder_for_hop"));
    assert_eq!(HopRow::TABLE, "hop");
    assert_eq!(RecipeRow::TABLE, "recipe");
}

#[test]
fn enum_strings_are_the_ones_the_database_stores() {
    assert_eq!(HopForm::Leaf.as_str(), "leaf");
    assert_eq!(HopForm::WetLeaf.as_str(), "leaf (wet)");
    assert_eq!(FermentableType::DryExtract.as_str(), "dry extract");
    assert_eq!(RecipeAdditionStage::Mash.as_str(), "add_to_mash");
    assert_eq!(YeastForm::Liquid.as_str(), "liquid");
}

#[test]
fn enums_round_trip_through_their_stored_strings() {
    for form in HopForm::ALL {
        assert_eq!(HopForm::from_str(form.as_str()).expect("round trip"), form);
    }
    for stage in RecipeAdditionStage::ALL {
        assert_eq!(RecipeAdditionStage::from_str(stage.as_str()).expect("round trip"), stage);
    }
    // Form means different things on Hop and on Yeast, so they must not be interchangeable.
    assert!(YeastForm::from_str("pellet").is_err());
    assert!(HopForm::from_str("liquid").is_err());
}

#[test]
fn an_unrecognised_string_is_an_error_naming_the_enumeration() {
    let error = HopForm::from_str("gravel").expect_err("not a hop form");
    assert_eq!(error.enumeration, "HopForm");
    assert_eq!(error.value, "gravel");
}

/// The generated enums must agree with `schema/enums.json`, which is what the extractor read out
/// of `src/model/`.
#[test]
fn generated_enums_match_the_extracted_mappings() {
    #[derive(serde::Deserialize)]
    struct Value {
        string: String,
    }
    #[derive(serde::Deserialize)]
    struct Mapping {
        values: Vec<Value>,
    }
    #[derive(serde::Deserialize)]
    struct Document {
        mappings: std::collections::BTreeMap<String, Mapping>,
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../schema/enums.json");
    let document: Document =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read enums")).expect("parse enums");

    let checks: [(&str, Vec<&str>); 4] = [
        ("Hop::formStringMapping", HopForm::ALL.iter().map(|m| m.as_str()).collect()),
        ("Yeast::formStringMapping", YeastForm::ALL.iter().map(|m| m.as_str()).collect()),
        ("Fermentable::typeStringMapping", FermentableType::ALL.iter().map(|m| m.as_str()).collect()),
        ("RecipeAddition::stageStringMapping", RecipeAdditionStage::ALL.iter().map(|m| m.as_str()).collect()),
    ];
    for (name, generated) in checks {
        let expected: Vec<&str> = document.mappings[name].values.iter().map(|value| value.string.as_str()).collect();
        assert_eq!(generated, expected, "{name}");
    }
}
