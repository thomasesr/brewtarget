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

// ── Bitterness ────────────────────────────────────────────────────────────────────────────────

use bt_calc::ibu::Formula;
use bt_calc::recipe_ibu::{IbuSettings, boil_addition_timing, kettle_sugars, recipe_ibus};
use bt_model::enums::HopForm;

/// A recipe with a three-step boil: fifteen minutes heating, an hour boiling, twenty cooling.
fn a_recipe_with_a_boil() -> Recipe {
    let mut recipe = a_simple_recipe();
    recipe.boil = Some((
        BoilRow { pre_boil_size_l: Some(23.66), ..Default::default() },
        vec![boil_step(65.0, 100.0, 15.0), boil_step(100.0, 100.0, 60.0), boil_step(100.0, 20.0, 20.0)],
    ));
    recipe
}

fn a_hop_addition(step: i32, add_at_time_mins: Option<f64>, grams: f64) -> Addition<HopInRecipeRow, HopRow> {
    let mut hop = HopRow { alpha_pct: Some(10.0), ..Default::default() };
    hop.set_form(Some(HopForm::Leaf));
    let mut addition =
        HopInRecipeRow { quantity: Some(grams / 1000.0), step: Some(step), add_at_time_mins, ..Default::default() };
    addition.set_unit(bt_measure::Unit::by_name("kilograms"));
    addition.set_stage(Some(RecipeAdditionStage::Boil));
    Addition { addition, ingredient: Some(hop) }
}

#[test]
fn a_boil_addition_is_timed_against_the_boil_proper() {
    let recipe = a_recipe_with_a_boil();

    // Added at the start of the boiling step: the whole hour.
    let whole = boil_addition_timing(&recipe, None, Some(2), None);
    assert!(!whole.post_boil);
    assert_eq!(whole.boil_proper_mins, 60.0);

    // Added with fifteen minutes left.
    let late = boil_addition_timing(&recipe, Some(15.0), Some(2), None);
    assert_eq!(late.boil_proper_mins, 15.0);

    // Added during the cooling step: after flameout, not in the boil at all.
    let whirlpool = boil_addition_timing(&recipe, None, Some(3), None);
    assert!(whirlpool.post_boil);
    assert_eq!(whirlpool.boil_proper_mins, 0.0);

    // Added during the heating step: it is in the kettle for the whole boil that follows.
    let first_wort = boil_addition_timing(&recipe, Some(0.0), Some(1), None);
    assert!(!first_wort.post_boil);
    assert_eq!(first_wort.boil_proper_mins, 60.0);
}

#[test]
fn a_recipe_with_hops_is_bitter_and_one_without_is_not() {
    let mut recipe = a_recipe_with_a_boil();
    let (none, per_addition) = recipe_ibus(&recipe, 1.050, IbuSettings::default());
    assert_eq!(none, 0.0);
    assert!(per_addition.is_empty());

    recipe.hops.push(a_hop_addition(2, None, 28.0));
    let (bitter, per_addition) = recipe_ibus(&recipe, 1.050, IbuSettings::default());
    assert!(bitter > 0.0, "{bitter}");
    assert_eq!(per_addition.len(), 1);
    assert!(close(per_addition[0], bitter, 1e-9), "one hop, so it is all of it");
    // 28 g of 10% hops boiled an hour into 19.87 L is a well-bittered beer.
    assert!((20.0..60.0).contains(&bitter), "{bitter}");
}

#[test]
fn a_later_addition_bitters_less_than_an_earlier_one() {
    let mut early = a_recipe_with_a_boil();
    early.hops.push(a_hop_addition(2, None, 28.0));
    let mut late = a_recipe_with_a_boil();
    late.hops.push(a_hop_addition(2, Some(10.0), 28.0));

    let (early_ibus, _) = recipe_ibus(&early, 1.050, IbuSettings::default());
    let (late_ibus, _) = recipe_ibus(&late, 1.050, IbuSettings::default());
    assert!(late_ibus < early_ibus, "{late_ibus} vs {early_ibus}");
}

/// A dry hop is for aroma, so it contributes no bitterness under any method.
#[test]
fn a_dry_hop_adds_no_bitterness() {
    let mut recipe = a_recipe_with_a_boil();
    let mut dry_hop = a_hop_addition(1, None, 56.0);
    dry_hop.addition.set_stage(Some(RecipeAdditionStage::Fermentation));
    recipe.hops.push(dry_hop);

    let (total, per_addition) = recipe_ibus(&recipe, 1.050, IbuSettings::default());
    assert_eq!(per_addition, vec![0.0]);
    assert_eq!(total, 0.0);
}

/// A mash hop contributes nothing by default, which is what the C++'s setting defaults to.
#[test]
fn a_mash_hop_contributes_only_if_the_setting_says_so() {
    let mut recipe = a_recipe_with_a_boil();
    let mut mash_hop = a_hop_addition(1, None, 28.0);
    mash_hop.addition.set_stage(Some(RecipeAdditionStage::Mash));
    recipe.hops.push(mash_hop);

    assert_eq!(recipe_ibus(&recipe, 1.050, IbuSettings::default()).0, 0.0);

    let generous = IbuSettings { mash_hop_adjustment: 0.5, ..Default::default() };
    assert!(recipe_ibus(&recipe, 1.050, generous).0 > 0.0);
}

/// Every method should agree a recipe is bitter, even if they disagree by how much.
#[test]
fn all_seven_formulae_produce_a_figure() {
    let mut recipe = a_recipe_with_a_boil();
    recipe.hops.push(a_hop_addition(2, None, 28.0));

    for formula in [
        Formula::Tinseth,
        Formula::Rager,
        Formula::Noonan,
        Formula::Garetz,
        Formula::Daniels,
        Formula::MIbu,
        Formula::Smph,
    ] {
        let settings = IbuSettings { formula, ..Default::default() };
        let (total, _) = recipe_ibus(&recipe, 1.050, settings);
        assert!(total > 0.0 && total < 200.0, "{formula:?} gave {total}");
    }
}

/// Sugar added part way through the boil thickens the wort only from that point on.
#[test]
fn sugar_added_during_the_boil_is_tracked_against_the_clock() {
    let mut recipe = a_recipe_with_a_boil();
    let mut late_sugar = FermentableRow { fine_grind_yield_pct: Some(100.0), ..Default::default() };
    late_sugar.set_ftype(Some(FermentableType::Sugar));
    let mut addition = FermentableInRecipeRow {
        quantity: Some(1.0),
        step: Some(2),
        add_at_time_mins: Some(10.0),
        ..Default::default()
    };
    addition.set_unit(bt_measure::Unit::by_name("kilograms"));
    addition.set_stage(Some(RecipeAdditionStage::Boil));
    recipe.fermentables.push(Addition { addition, ingredient: Some(late_sugar) });

    let sugars = kettle_sugars(&recipe);
    // The grain was mashed, so it is in the kettle from the start; the sugar arrives later.
    assert!(sugars.at_start_of_boil_kg > 0.0);
    assert_eq!(sugars.in_boil_additions.len(), 1);
    // With fifty minutes left the late sugar is not in yet; with five minutes left it is.
    assert!(sugars.in_kettle_kg(50.0) < sugars.in_kettle_kg(5.0));
}

#[test]
fn calculate_reports_bitterness_alongside_everything_else() {
    let mut recipe = a_recipe_with_a_boil();
    recipe.hops.push(a_hop_addition(2, None, 28.0));
    let calculated = calculate(&recipe, ColorFormula::Morey, None);

    assert!(calculated.ibus > 0.0);
    assert_eq!(calculated.ibus_per_addition.len(), 1);
    assert!(close(calculated.ibus_per_addition.iter().sum::<f64>(), calculated.ibus, 1e-9));
    assert!(calculated.og > 1.0, "and the rest still works");
}
