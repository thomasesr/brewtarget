// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/recipe.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Checks on the recipe-level calculations.
//!
//! `RecipeCalcTests::testBoilStepPhases` is mirrored exactly -- it is self-contained and covers
//! eight real boil shapes.  The two gravity tests in that file,
//! `recipeCalcTest_allGrain` and `postBoilLossOgTest`, both begin with `return;` and so assert
//! nothing upstream; the gravity checks here are built from scratch instead.

use bt_calc::recipe::*;
use bt_calc::{ColorFormula, mash};
use bt_model::enums::{FermentableType, RecipeAdditionStage};
use bt_model::recipe::{Addition, Recipe};
use bt_model::tables::*;

fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

/// A boil step, where a negative temperature means "not recorded".
fn boil_step(start_temp_c: f64, end_temp_c: f64, step_time_mins: f64) -> BoilStepRow {
    BoilStepRow {
        start_temp_c: (start_temp_c >= 0.0).then_some(start_temp_c),
        end_temp_c: (end_temp_c >= 0.0).then_some(end_temp_c),
        step_time_mins: Some(step_time_mins),
        ..Default::default()
    }
}

/// Mirrors `RecipeCalcTests::testBoilStepPhases`, case for case.
#[test]
fn boil_steps_are_assigned_to_the_right_phase() {
    use StepPhase::{BoilProper, PostBoil, PreBoil};

    for (steps, expected, description) in [
        (
            vec![(65.0, 100.0, 15.0), (100.0, 100.0, 60.0), (100.0, 20.0, 20.0)],
            vec![PreBoil, BoilProper, PostBoil],
            "Standard boil",
        ),
        (vec![(-1.0, -1.0, 60.0), (-1.0, 20.0, 20.0)], vec![BoilProper, PostBoil], "Temperatures not set"),
        (
            vec![(65.0, 100.0, 15.0), (100.0, 100.0, 60.0), (100.0, 92.0, 10.0), (92.0, 20.0, 15.0)],
            vec![PreBoil, BoilProper, PostBoil, PostBoil],
            "Hot whirlpool",
        ),
        (
            vec![(100.0, 100.0, 60.0), (100.0, 92.0, 5.0), (92.0, 92.0, 20.0)],
            vec![BoilProper, PostBoil, PostBoil],
            "Hop stand after cooling step",
        ),
        (
            vec![(70.0, 95.0, 15.0), (95.0, 95.0, 60.0), (93.0, 93.0, 20.0)],
            vec![PreBoil, BoilProper, PostBoil],
            "Whirlpool at altitude",
        ),
        (
            vec![(100.0, 100.0, 60.0), (100.0, 100.0, 10.0), (100.0, 20.0, 20.0)],
            vec![BoilProper, BoilProper, PostBoil],
            "Held at boiling after boil",
        ),
        (
            vec![(81.0, 81.0, 30.0), (81.0, 100.0, 10.0), (100.0, 100.0, 30.0)],
            vec![BoilProper, BoilProper, BoilProper],
            "Split boil",
        ),
        (vec![(65.0, 78.0, 15.0), (78.0, 20.0, 15.0)], vec![PreBoil, PreBoil], "No boil proper"),
    ] {
        let rows: Vec<BoilStepRow> = steps.into_iter().map(|(s, e, t)| boil_step(s, e, t)).collect();
        assert_eq!(step_phases(&rows), expected, "{description}");
    }
}

#[test]
fn only_the_boil_proper_counts_towards_the_boil_time() {
    let recipe = Recipe {
        boil: Some((
            BoilRow::default(),
            // Fifteen minutes heating, an hour boiling, twenty minutes cooling.
            vec![boil_step(65.0, 100.0, 15.0), boil_step(100.0, 100.0, 60.0), boil_step(100.0, 20.0, 20.0)],
        )),
        ..Default::default()
    };
    assert_eq!(boil_time_mins(&recipe), 60.0);

    // A recipe with no boil at all boils for no time.
    assert_eq!(boil_time_mins(&Recipe::default()), 0.0);
}

/// 5 kg of pale malt at 80% yield and 4% moisture, mashed at 70% efficiency into 19.87 litres,
/// with a yeast that attenuates 75%.
fn a_simple_recipe() -> Recipe {
    let fermentable = FermentableRow {
        name: Some("Maris Otter".to_owned()),
        fine_grind_yield_pct: Some(80.0),
        moisture: Some(4.0),
        color_lovibond: Some(3.0),
        ..Default::default()
    };
    let mut fermentable_addition = FermentableInRecipeRow { quantity: Some(5.0), ..Default::default() };
    fermentable_addition.set_unit(bt_measure::Unit::by_name("kilograms"));
    fermentable_addition.set_stage(Some(RecipeAdditionStage::Mash));
    let mut fermentable_row = fermentable.clone();
    fermentable_row.set_ftype(Some(FermentableType::Grain));

    let yeast = YeastRow { attenuation_min_pct: Some(70.0), attenuation_max_pct: Some(80.0), ..Default::default() };

    Recipe {
        row: RecipeRow { batch_size: Some(19.87), efficiency: Some(70.0), ..Default::default() },
        fermentables: vec![Addition { addition: fermentable_addition, ingredient: Some(fermentable_row) }],
        yeasts: vec![Addition { addition: YeastInRecipeRow::default(), ingredient: Some(yeast) }],
        ..Default::default()
    }
}

/// Expected values worked out independently from the formulae, not read off the implementation.
#[test]
fn a_simple_all_grain_recipe_comes_out_where_it_should() {
    let recipe = a_simple_recipe();
    let calculated = calculate(&recipe, ColorFormula::Morey, None);

    // 5 kg of grain, all of it in the mash.
    assert!(close(calculated.grains_kg, 5.0, 1e-12));
    assert!(close(calculated.grains_in_mash_kg, 5.0, 1e-12));

    // 5 kg at 80% yield less 4% moisture is 3.84 kg of sucrose, of which 70% is extracted.
    let sugars = sugar_totals(&recipe);
    assert!(close(sugars.sugars_from_starch_all_kg, 3.84, 1e-9), "{}", sugars.sugars_from_starch_all_kg);
    assert_eq!(sugars.existing_sugars_all_kg, 0.0, "grain is not an existing sugar");

    assert!(close(calculated.og, 1.052092, 1e-5), "OG {}", calculated.og);
    // The yeast's typical attenuation is the midpoint of 70 and 80.
    assert!(close(calculated.fg, 1.013023, 1e-5), "FG {}", calculated.fg);
    // Nothing here is unfermentable, so both gravities are the ones the yeast sees.
    assert_eq!(calculated.og, calculated.og_fermentable);
    assert_eq!(calculated.fg, calculated.fg_fermentable);

    assert!(close(calculated.color_mcu, 6.300003, 1e-5), "MCU {}", calculated.color_mcu);
    assert!(close(calculated.color_srm, 5.273451, 1e-5), "SRM {}", calculated.color_srm);

    // Around 5% for a beer of that gravity drop.
    assert!((4.5..5.5).contains(&calculated.abv_pct), "ABV {}", calculated.abv_pct);
    assert!(calculated.calories_per_liter > 0.0);
}

/// A fermentable that is only partly fermentable -- lactose is the obvious case -- finishes
/// higher without that being credited to the alcohol.
#[test]
fn an_unfermentable_sugar_raises_the_final_gravity_but_not_the_alcohol() {
    let mut with_lactose = a_simple_recipe();
    let mut lactose = FermentableRow {
        name: Some("Lactose".to_owned()),
        fine_grind_yield_pct: Some(100.0),
        fermentable_fraction_pct: Some(0.0),
        ..Default::default()
    };
    lactose.set_ftype(Some(FermentableType::Sugar));
    let mut addition = FermentableInRecipeRow { quantity: Some(0.5), ..Default::default() };
    addition.set_unit(bt_measure::Unit::by_name("kilograms"));
    addition.set_stage(Some(RecipeAdditionStage::Boil));
    with_lactose.fermentables.push(Addition { addition, ingredient: Some(lactose) });

    let plain = calculate(&a_simple_recipe(), ColorFormula::Morey, None);
    let sweet = calculate(&with_lactose, ColorFormula::Morey, None);

    assert!(sweet.og > plain.og, "the extra sugar raises the original gravity");
    assert!(sweet.fg > plain.fg, "and it is still there at the end");
    // The gravity the yeast sees is what drives the alcohol, so the lactose does not inflate it.
    assert!(sweet.fg > sweet.fg_fermentable, "the hydrometer reads higher than the yeast is responsible for");
}

#[test]
fn a_recipe_with_no_yeast_does_not_ferment() {
    let mut no_yeast = a_simple_recipe();
    no_yeast.yeasts.clear();
    let calculated = calculate(&no_yeast, ColorFormula::Morey, None);
    assert_eq!(calculated.fg, calculated.og, "nothing to attenuate it");
    assert_eq!(calculated.abv_pct, 0.0);
}

/// A cooler mash makes more fermentable wort, so the beer finishes lower and stronger.
#[test]
fn the_mash_schedule_moves_the_final_gravity() {
    let recipe = a_simple_recipe();
    let multiplier_for = |temp_c: f64| {
        let profile = mash::build_profile(&[mash::Step {
            kind: mash::StepType::Infusion,
            step_temp_c: temp_c,
            end_temp_c: None,
            step_time_mins: 60.0,
            ramp_time_mins: None,
        }]);
        mash::attenuation_multiplier(&profile, mash::enzyme_levels(None, None))
    };

    let cool = calculate(&recipe, ColorFormula::Morey, multiplier_for(62.0));
    let hot = calculate(&recipe, ColorFormula::Morey, multiplier_for(70.0));
    let reference = calculate(&recipe, ColorFormula::Morey, multiplier_for(66.0));

    assert!(cool.fg < reference.fg, "a cooler mash ferments further");
    assert!(hot.fg > reference.fg, "a hotter one leaves more behind");
    assert!(cool.abv_pct > hot.abv_pct);
    // The mash changes nothing before fermentation.
    assert_eq!(cool.og, hot.og);
}
