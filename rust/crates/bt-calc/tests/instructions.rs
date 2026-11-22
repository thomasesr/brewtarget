// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/instructions.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Writing a brew day sheet.
//!
//! What matters is the order and the sense of it: a brewer reads these top to bottom and does
//! them in that order, so an instruction in the wrong place is worse than a missing one.

use bt_calc::instructions::generate;
use bt_model::enums::{MashStepType, RecipeAdditionStage};
use bt_model::recipe::{Addition, Recipe};
use bt_model::tables::*;

fn malt(name: &str, kg: f64, stage: RecipeAdditionStage) -> Addition<FermentableInRecipeRow, FermentableRow> {
    Addition {
        addition: FermentableInRecipeRow {
            quantity: Some(kg),
            unit: Some("kilograms".to_owned()),
            stage: Some(stage.as_str().to_owned()),
            ..FermentableInRecipeRow::default()
        },
        ingredient: Some(FermentableRow {
            name: Some(name.to_owned()),
            ftype: Some("grain".to_owned()),
            fine_grind_yield_pct: Some(80.0),
            ..FermentableRow::default()
        }),
    }
}

fn hop(name: &str, kg: f64, step: i32, at_time_mins: f64) -> Addition<HopInRecipeRow, HopRow> {
    Addition {
        addition: HopInRecipeRow {
            quantity: Some(kg),
            unit: Some("kilograms".to_owned()),
            stage: Some(RecipeAdditionStage::Boil.as_str().to_owned()),
            step: Some(step),
            add_at_time_mins: Some(at_time_mins),
            ..HopInRecipeRow::default()
        },
        ingredient: Some(HopRow { name: Some(name.to_owned()), alpha_pct: Some(5.0), ..HopRow::default() }),
    }
}

/// A single-infusion all-grain recipe: mash, boil, ferment.
fn recipe() -> Recipe {
    Recipe {
        row: RecipeRow {
            name: Some("Test".to_owned()),
            batch_size: Some(20.0),
            efficiency: Some(75.0),
            ..RecipeRow::default()
        },
        fermentables: vec![malt("Pale Malt", 5.0, RecipeAdditionStage::Mash)],
        hops: vec![hop("Bittering", 0.05, 2, 60.0), hop("Aroma", 0.025, 3, 5.0)],
        yeasts: vec![Addition {
            addition: YeastInRecipeRow {
                quantity: Some(0.035),
                unit: Some("kilograms".to_owned()),
                stage: Some(RecipeAdditionStage::Fermentation.as_str().to_owned()),
                step: Some(1),
                ..YeastInRecipeRow::default()
            },
            ingredient: Some(YeastRow {
                name: Some("London Ale".to_owned()),
                ytype: Some("ale".to_owned()),
                ..YeastRow::default()
            }),
        }],
        mash: Some((
            MashRow { name: Some("Single infusion".to_owned()), ..MashRow::default() },
            vec![MashStepRow {
                name: Some("Saccharification".to_owned()),
                mstype: Some(MashStepType::Infusion.as_str().to_owned()),
                step_number: Some(1),
                step_temp_c: Some(66.0),
                infuse_temp_c: Some(72.0),
                amount_l: Some(16.0),
                step_time_mins: Some(60.0),
                ..MashStepRow::default()
            }],
        )),
        boil: Some((
            BoilRow { name: Some("Boil".to_owned()), pre_boil_size_l: Some(25.0), ..BoilRow::default() },
            vec![
                BoilStepRow {
                    step_number: Some(1),
                    start_temp_c: Some(70.0),
                    end_temp_c: Some(100.0),
                    ..BoilStepRow::default()
                },
                BoilStepRow {
                    step_number: Some(2),
                    start_temp_c: Some(100.0),
                    end_temp_c: Some(100.0),
                    step_time_mins: Some(60.0),
                    ..BoilStepRow::default()
                },
                BoilStepRow {
                    step_number: Some(3),
                    start_temp_c: Some(100.0),
                    end_temp_c: Some(20.0),
                    ..BoilStepRow::default()
                },
            ],
        )),
        fermentation: Some((
            FermentationRow { name: Some("Ferment".to_owned()), ..FermentationRow::default() },
            vec![FermentationStepRow {
                step_number: Some(1),
                start_temp_c: Some(19.0),
                step_time_mins: Some(14400.0),
                ..FermentationStepRow::default()
            }],
        )),
        ..Recipe::default()
    }
}

/// Everything is numbered from one, in the order a brewer does it, with nothing blank.
#[test]
fn the_sheet_is_in_the_order_it_is_to_be_done() {
    let sheet = generate(&recipe());
    assert!(!sheet.is_empty(), "a recipe with a mash and a boil has something to do");

    for (index, instruction) in sheet.iter().enumerate() {
        assert_eq!(instruction.step_number, index as i32 + 1, "numbered from one, in order");
        assert!(!instruction.name.trim().is_empty(), "{instruction:?}");
        assert!(!instruction.directions.trim().is_empty(), "{instruction:?}");
    }

    let names: Vec<&str> = sheet.iter().map(|instruction| instruction.name.as_str()).collect();
    // The mash comes before the boil, which comes before the fermentation.
    let position = |name: &str| names.iter().position(|found| *found == name);
    let grains = position("Add grains").expect("grains go in");
    let start_boil = position("Start boil").expect("the boil starts");
    let flameout = position("Flameout").expect("the boil ends");
    let pitch = position("Pitch yeast").expect("yeast is pitched");
    assert!(grains < start_boil, "{names:?}");
    assert!(start_boil < flameout, "{names:?}");
    assert!(flameout < pitch, "{names:?}");
}

/// Hop additions are ordered by how long they are in for, longest first -- which is how a
/// brewer adds them, and is why the sort inverts the interval.
#[test]
fn the_longer_an_addition_is_in_the_earlier_it_comes() {
    let sheet = generate(&recipe());
    let bittering = sheet.iter().position(|step| step.name == "Add Bittering").expect("the bittering hop");
    let aroma = sheet.iter().position(|step| step.name == "Add Aroma").expect("the aroma hop");
    assert!(bittering < aroma, "60 minutes before 5: {sheet:#?}");
}

/// The mash step says what to add, how hot, and how long to hold it.
#[test]
fn a_mash_step_says_what_to_do_with_it() {
    let sheet = generate(&recipe());
    let step = sheet.iter().find(|step| step.name.starts_with("Infusion")).expect("the infusion step");
    assert!(step.directions.contains("16.0 L"), "{}", step.directions);
    assert!(step.directions.contains("72.0 °C"), "{}", step.directions);
    assert!(step.directions.contains("66.0 °C"), "{}", step.directions);
    assert!(step.directions.contains("Hold for 60 min"), "{}", step.directions);
}

/// A recipe with nothing in it has nothing to say, rather than a sheet of empty steps.
#[test]
fn an_empty_recipe_has_an_empty_sheet() {
    assert!(generate(&Recipe::default()).is_empty());
}

/// Extract recipes have no mash, so the sheet starts at the boil.
#[test]
fn an_extract_recipe_starts_at_the_boil() {
    let mut recipe = recipe();
    recipe.mash = None;
    recipe.fermentables = vec![malt("Dry Extract", 3.0, RecipeAdditionStage::Boil)];
    let sheet = generate(&recipe);

    let names: Vec<&str> = sheet.iter().map(|instruction| instruction.name.as_str()).collect();
    assert!(!names.contains(&"Add grains"), "no mash, so no grains to add to it: {names:?}");
    assert!(names.contains(&"Start boil"), "{names:?}");
}

/// Grains and water come before the step that uses them.
///
/// The C++ orders these by the step's *position* in the mash, which is always set, not by the
/// `step_number` column, which often is not.  Taking the column left every mash instruction
/// tied, and the tiebreak -- time remaining -- then put "Add grains" after the mash step.
#[test]
fn the_grains_go_in_before_the_mash_step_that_uses_them() {
    let mut recipe = recipe();
    // As the shipped content has them: no step number recorded.
    if let Some((_, steps)) = recipe.mash.as_mut() {
        for step in steps.iter_mut() {
            step.step_number = None;
        }
        steps.push(MashStepRow {
            name: Some("Mash out".to_owned()),
            mstype: Some(MashStepType::Temperature.as_str().to_owned()),
            step_temp_c: Some(76.0),
            step_time_mins: Some(10.0),
            ..MashStepRow::default()
        });
    }

    let sheet = generate(&recipe);
    let names: Vec<&str> = sheet.iter().map(|instruction| instruction.name.as_str()).collect();
    let position = |name: &str| {
        names.iter().position(|found| *found == name).unwrap_or_else(|| panic!("{name} missing from {names:?}"))
    };

    assert!(position("Add grains") < position("Infusion - Saccharification"), "{names:?}");
    assert!(position("Heat water") < position("Infusion - Saccharification"), "{names:?}");
    // ...and the mash steps are still in their own order.
    assert!(position("Infusion - Saccharification") < position("Temperature - Mash out"), "{names:?}");
}
