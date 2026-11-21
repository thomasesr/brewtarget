// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/recipe.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! What a recipe works out to: gravity, alcohol, colour, bitterness, calories.
//!
//! Ported from the `recalc*` functions in `src/model/Recipe.cpp`.  Those mutate the recipe and
//! emit change notifications; here they are plain functions returning a value, because nothing in
//! this crate should be deciding when a recalculation is due.
//!
//! The order the C++ runs them in is a dependency order, not a preference: grains feed the volume
//! estimates, which feed the colour and the gravities, which feed the alcohol and the calories.
//! [`calculate`] keeps that order.

use bt_measure::Unit;
use bt_model::enums::{FermentableType, RecipeAdditionStage};
use bt_model::recipe::{Addition, IngredientAmount, Recipe};
use bt_model::tables::{EquipmentRow, FermentableInRecipeRow, FermentableRow};

use crate::algorithms;
use crate::color::{ColorFormula, mcu_to_srm};
use crate::constant;

/// `Equipment::default_*` in `src/model/Equipment.h`.
const DEFAULT_KETTLE_EVAPORATION_PER_HOUR_L: f64 = 4.0;
/// Note this is not `PhysicalConstants::grainAbsorption_Lkg`, which is 1.085; see [`volumes`].
const DEFAULT_GRAIN_ABSORPTION_L_KG: f64 = 1.086;
const DEFAULT_TOP_UP_KETTLE_L: f64 = 0.0;
const DEFAULT_TOP_UP_WATER_L: f64 = 0.0;

/// `Yeast::DefaultAttenuation_pct`, used when a recipe has yeast but none of it says how far it
/// attenuates.
const DEFAULT_ATTENUATION_PCT: f64 = 75.0;

/// Everything a recipe works out to.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Calculated {
    pub grains_kg: f64,
    pub grains_in_mash_kg: f64,
    pub wort_from_mash_l: f64,
    pub boil_volume_l: f64,
    pub post_boil_volume_l: f64,
    pub final_volume_l: f64,
    pub color_mcu: f64,
    pub color_srm: f64,
    pub og: f64,
    pub fg: f64,
    /// Gravity counting only the sugars the yeast can work on, which is what the alcohol
    /// calculation uses.
    pub og_fermentable: f64,
    pub fg_fermentable: f64,
    pub abv_pct: f64,
    pub calories_per_liter: f64,
}

/// Sugar in the recipe, split the four ways `recalcOgFg` needs it.
///
/// Ported from `Recipe::sugarTotals`.  Sugar that came with the ingredient -- in an extract, in
/// honey -- goes into the wort whatever the mash does, so it is not affected by mash efficiency.
/// Sugar that has to be converted from starch is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sugars {
    pub existing_sugars_all_kg: f64,
    pub existing_sugars_non_fermentable_kg: f64,
    pub sugars_from_starch_all_kg: f64,
    pub sugars_from_starch_non_fermentable_kg: f64,
}

/// Whether a fermentable arrives with its sugar already made.
///
/// Ported from `Fermentable::isExistingSugar`.
fn is_existing_sugar(fermentable_type: Option<FermentableType>) -> bool {
    matches!(
        fermentable_type,
        Some(
            FermentableType::Sugar
                | FermentableType::Extract
                | FermentableType::DryExtract
                | FermentableType::Fruit
                | FermentableType::Juice
                | FermentableType::Honey
        )
    )
}

/// The fine grind yield, worked out from whichever figure the ingredient happens to carry.
///
/// Ported from `Fermentable::effectiveFineGrindYield_pct`.  Maltsters publish one of three
/// things, so try them in the order the C++ does and derive the others.
pub fn effective_fine_grind_yield_pct(fermentable: &FermentableRow) -> Option<f64> {
    if let Some(fine) = fermentable.fine_grind_yield_pct {
        return Some(fine);
    }
    if let Some(coarse) = fermentable.coarse_grind_yield_pct {
        // The fine/coarse difference is fine minus coarse, so it adds back on.
        return Some(coarse + fermentable.coarse_fine_diff.unwrap_or(0.0));
    }
    if let Some(potential_sg) = fermentable.potential_yield_sg {
        // Potential yield is the gravity of one pound in one US gallon, as weighed, so the
        // moisture has to come back out of it.
        let dry_fraction = 1.0 - fermentable.moisture.unwrap_or(0.0) / 100.0;
        if dry_fraction <= 0.0 {
            return None;
        }
        return Some((potential_sg - 1.0) / 0.046 * 100.0 / dry_fraction);
    }
    None
}

/// How much sucrose one fermentable addition is worth.
///
/// Ported from `RecipeAdditionFermentable::equivSucrose_kg`.  A grain added anywhere but the mash
/// is a steeped grain, which gives up much less of itself.
pub fn equiv_sucrose_kg(addition: &Addition<FermentableInRecipeRow, FermentableRow>) -> f64 {
    let Some(fermentable) = &addition.ingredient else {
        return 0.0;
    };
    let Some(amount) = addition.addition.canonical_amount() else {
        return 0.0;
    };
    // Only a mass can be turned into an equivalent mass of sucrose.
    if amount.unit != Unit::by_name("kilograms").expect("the kilogram") {
        return 0.0;
    }

    let yield_pct = effective_fine_grind_yield_pct(fermentable).unwrap_or(0.0);
    let dry_fraction = 1.0 - fermentable.moisture.unwrap_or(0.0) / 100.0;
    let sucrose_kg = amount.quantity * yield_pct * dry_fraction / 100.0;

    let is_grain = addition.ingredient.as_ref().and_then(|f| f.ftype().ok().flatten()) == Some(FermentableType::Grain);
    let in_mash = addition.addition.stage().ok().flatten() == Some(RecipeAdditionStage::Mash);
    if is_grain && !in_mash { 0.60 * sucrose_kg } else { sucrose_kg }
}

/// Split the recipe's sugar into what the mash affects and what it does not.
pub fn sugar_totals(recipe: &Recipe) -> Sugars {
    let mut totals = Sugars::default();
    for addition in &recipe.fermentables {
        let Some(fermentable) = &addition.ingredient else {
            continue;
        };
        let sucrose_kg = equiv_sucrose_kg(addition);
        // A fermentable may be only partly fermentable -- lactose is the obvious case.
        let unfermentable_share = 1.0 - fermentable.fermentable_fraction_pct.unwrap_or(100.0) / 100.0;

        if is_existing_sugar(fermentable.ftype().ok().flatten()) {
            totals.existing_sugars_all_kg += sucrose_kg;
            totals.existing_sugars_non_fermentable_kg += sucrose_kg * unfermentable_share;
        } else {
            totals.sugars_from_starch_all_kg += sucrose_kg;
            totals.sugars_from_starch_non_fermentable_kg += sucrose_kg * unfermentable_share;
        }
    }
    totals
}

/// Whether the equipment mashes in the kettle rather than in a separate tun.
///
/// Ported from `Equipment::mashesInKettle`: no mash tun volume means there is no mash tun.
fn mashes_in_kettle(equipment: &EquipmentRow) -> bool {
    equipment.mash_tun_volume_l.unwrap_or(0.0) <= 0.0
}

/// What stays behind in the mash tun.
///
/// Ported from `Equipment::effectiveMashTunLoss_l`.  Mashing in the kettle means there is no
/// separate vessel to lose wort in, so a recorded mash tun loss does not apply.
fn effective_mash_tun_loss_l(equipment: &EquipmentRow) -> f64 {
    if mashes_in_kettle(equipment) { 0.0 } else { equipment.mash_tun_loss_l.unwrap_or(0.0) }
}

/// What is lost between the mash and the kettle.
///
/// Ported from `Equipment::getLauteringDeadspaceLoss_l`.  A lauter tun's deadspace only counts
/// when there is a lauter tun.
pub fn lautering_deadspace_loss_l(equipment: &EquipmentRow) -> f64 {
    effective_mash_tun_loss_l(equipment)
        + if equipment.lauter_tun_volume_l.unwrap_or(0.0) > 0.0 {
            equipment.lauter_tun_deadspace_loss_l.unwrap_or(0.0)
        } else {
            0.0
        }
}

/// How much wort is left after boiling for a while.
///
/// Ported from `Equipment::wortEndOfBoil_l`.
pub fn wort_end_of_boil_l(equipment: &EquipmentRow, kettle_wort_l: f64, boil_time_mins: f64) -> f64 {
    kettle_wort_l
        - (boil_time_mins / 60.0)
            * equipment.kettle_evaporation_per_hour_l.unwrap_or(DEFAULT_KETTLE_EVAPORATION_PER_HOUR_L)
}

/// Water added by the infusion steps of the mash.
///
/// Ported from `Mash::totalMashWater_l` and `MashStep::isInfusion`, which counts the sparges as
/// well as the infusions proper -- they all put water in.
pub fn total_mash_water_l(recipe: &Recipe) -> f64 {
    use bt_model::enums::MashStepType;
    recipe
        .mash_steps()
        .iter()
        .filter(|step| {
            matches!(
                step.mstype().ok().flatten(),
                Some(MashStepType::Infusion | MashStepType::BatchSparge | MashStepType::FlySparge)
            )
        })
        .filter_map(|step| step.amount_l)
        .sum()
}

/// Grain in the recipe, and grain in the mash.
///
/// Ported from `recalcGrains`.  Only grain counts, and only when it is measured by weight -- the
/// C++ warns and skips a grain given as a volume, since there is no sensible mass to add up.
pub fn grains(recipe: &Recipe) -> (f64, f64) {
    let kilograms = Unit::by_name("kilograms").expect("the kilogram");
    let mut total_kg = 0.0;
    let mut in_mash_kg = 0.0;
    for addition in &recipe.fermentables {
        if addition.ingredient.as_ref().and_then(|f| f.ftype().ok().flatten()) != Some(FermentableType::Grain) {
            continue;
        }
        let Some(amount) = addition.addition.canonical_amount() else {
            continue;
        };
        if amount.unit != kilograms {
            continue; // measured by volume, so there is no weight to add up
        }
        total_kg += amount.quantity;
        if addition.addition.stage().ok().flatten() == Some(RecipeAdditionStage::Mash) {
            in_mash_kg += amount.quantity;
        }
    }
    (total_kg, in_mash_kg)
}

/// Volumes through the brew: out of the mash, into the kettle, out of the boil, into the glass.
///
/// Ported from `recalcVolumeEstimates`.  Without equipment there is very little to go on, and
/// the C++ says as much in its own comment -- the final volume becomes the boil volume less four
/// litres, which it describes as shooting in the dark.  That guess is kept rather than replaced
/// with a different one.
pub fn volumes(recipe: &Recipe, grains_in_mash_kg: f64) -> (f64, f64, f64, f64) {
    let equipment = recipe.equipment.as_ref();
    let batch_size_l = recipe.row.batch_size.unwrap_or(0.0);

    let wort_from_mash_l = if recipe.mash.is_none() {
        0.0
    } else {
        let water_added_l = total_mash_water_l(recipe);
        // The two defaults are not the same number: Equipment::default_grainAbsorption_LKg is
        // 1.086 and PhysicalConstants::grainAbsorption_Lkg is 1.085, and the C++ picks between
        // them on whether there is equipment at all rather than on whether it has a figure.
        let absorption_l_kg = match equipment {
            Some(equipment) => equipment.grain_absorption_lkg.unwrap_or(DEFAULT_GRAIN_ABSORPTION_L_KG),
            None => constant("grainAbsorption_Lkg"),
        };
        water_added_l - absorption_l_kg * grains_in_mash_kg
    };

    let mut boil_volume_l = match equipment {
        Some(equipment) => {
            wort_from_mash_l - lautering_deadspace_loss_l(equipment)
                + equipment.top_up_kettle.unwrap_or(DEFAULT_TOP_UP_KETTLE_L)
        }
        None => wort_from_mash_l,
    };
    if boil_volume_l <= 0.0 {
        // No mash to work from, so fall back on what the boil says it holds.
        boil_volume_l = recipe.boil.as_ref().and_then(|(boil, _)| boil.pre_boil_size_l).unwrap_or(0.0);
    }

    let boil_time_mins = boil_time_mins(recipe);
    let post_boil_volume_l = match (equipment, recipe.boil.as_ref()) {
        (Some(equipment), Some(_)) => wort_end_of_boil_l(equipment, boil_volume_l, boil_time_mins),
        _ => batch_size_l, // give up
    };

    let final_volume_l = match equipment {
        Some(equipment) => {
            post_boil_volume_l + equipment.top_up_water.unwrap_or(DEFAULT_TOP_UP_WATER_L)
                - equipment.kettle_trub_chiller_loss_l.unwrap_or(0.0)
        }
        None => boil_volume_l - 4.0,
    };

    (wort_from_mash_l, boil_volume_l, post_boil_volume_l, final_volume_l)
}

/// Colour in malt colour units, and the SRM that approximates.
///
/// Ported from `recalcColor_mcu`.  Malt colour units are pounds of malt times degrees Lovibond
/// per US gallon, which is why a metric recipe has to be converted into those units first.
pub fn color(recipe: &Recipe, final_volume_no_losses_l: f64, formula: ColorFormula) -> (f64, f64) {
    const KILOGRAMS_TO_POUNDS: f64 = 1.0 / 0.45359237;
    const LITERS_TO_US_GALLONS: f64 = 1.0 / 3.785411784;
    const KG_PER_LITER_TO_POUNDS_PER_GALLON: f64 = KILOGRAMS_TO_POUNDS / LITERS_TO_US_GALLONS;

    if final_volume_no_losses_l <= 0.0 {
        return (0.0, 0.0);
    }
    let common_multiplier = KG_PER_LITER_TO_POUNDS_PER_GALLON / final_volume_no_losses_l;
    let kilograms = Unit::by_name("kilograms").expect("the kilogram");

    let mut color_mcu = 0.0;
    for addition in &recipe.fermentables {
        let Some(fermentable) = &addition.ingredient else {
            continue;
        };
        let Some(amount) = addition.addition.canonical_amount() else {
            continue;
        };
        if amount.unit != kilograms {
            continue; // a fermentable given as a volume has no weight to colour with
        }
        color_mcu += amount.quantity * fermentable.color_lovibond.unwrap_or(0.0) * common_multiplier;
    }
    (color_mcu, mcu_to_srm(color_mcu, formula))
}

/// The attenuation to expect, before the mash is taken into account.
///
/// Ported from the yeast loop in `recalcOgFg`: the most attenuative yeast in the recipe sets the
/// figure, and a recipe with yeast but no figure gets an average one rather than none.
fn attenuation_pct(recipe: &Recipe) -> f64 {
    let mut attenuation = 0.0f64;
    for addition in &recipe.yeasts {
        let value =
            addition.addition.attenuation_pct.or_else(|| addition.ingredient.as_ref().map(attenuation_typical_pct));
        if let Some(value) = value {
            attenuation = attenuation.max(value);
        }
    }
    if !recipe.yeasts.is_empty() && attenuation <= 0.0 { DEFAULT_ATTENUATION_PCT } else { attenuation }
}

/// A yeast's typical attenuation, which the C++ takes as the midpoint of its published range.
///
/// Ported from `Yeast::attenuationTypical_pct`: without both ends of the range there is nothing
/// to average, so an average yeast is assumed.
pub fn attenuation_typical_pct(yeast: &bt_model::tables::YeastRow) -> f64 {
    match (yeast.attenuation_min_pct, yeast.attenuation_max_pct) {
        (Some(min), Some(max)) => (min + max) / 2.0,
        _ => DEFAULT_ATTENUATION_PCT,
    }
}

/// Where in the boil a step sits.
///
/// Ported from `Boil::StepPhase`.  A boil is not only the boil: the wort has to get up to
/// temperature first and comes back down afterwards, and only the middle counts as boiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPhase {
    PreBoil,
    BoilProper,
    PostBoil,
}

/// `Boil::minimumBoilTemperature_c`.
const MINIMUM_BOIL_TEMPERATURE_C: f64 = 81.0;

/// A step with no temperature recorded is taken to be at the boil, which is what the C++'s
/// infinity sentinel amounts to.
fn start_temp_for_comparison(step: &bt_model::tables::BoilStepRow) -> f64 {
    step.start_temp_c.unwrap_or(f64::INFINITY)
}

fn end_temp_for_comparison(step: &bt_model::tables::BoilStepRow) -> f64 {
    step.end_temp_c.unwrap_or(f64::INFINITY)
}

/// Whether a step stays at the boil throughout.
///
/// Ported from `Boil::holdsAtBoil`: hot at both ends, and not cooling as it goes.
fn holds_at_boil(step: &bt_model::tables::BoilStepRow) -> bool {
    let start = start_temp_for_comparison(step);
    let end = end_temp_for_comparison(step);
    start >= MINIMUM_BOIL_TEMPERATURE_C && end >= MINIMUM_BOIL_TEMPERATURE_C && end >= start
}

/// Work out which phase each boil step belongs to.
///
/// Ported from `Boil::stepPhases`.  The phases only go forwards: once the wort has come off the
/// boil it does not go back on, so a later hot step is still post-boil.
pub fn step_phases(steps: &[bt_model::tables::BoilStepRow]) -> Vec<StepPhase> {
    let mut phases = Vec::with_capacity(steps.len());
    let mut current = StepPhase::PreBoil;
    let mut previous: Option<&bt_model::tables::BoilStepRow> = None;

    for step in steps {
        match current {
            StepPhase::PreBoil => {
                if holds_at_boil(step) {
                    current = StepPhase::BoilProper;
                }
            }
            StepPhase::BoilProper => {
                if let Some(previous) = previous
                    && (!holds_at_boil(step) || start_temp_for_comparison(step) < end_temp_for_comparison(previous))
                {
                    current = StepPhase::PostBoil;
                }
            }
            StepPhase::PostBoil => {}
        }
        phases.push(current);
        previous = Some(step);
    }
    phases
}

/// How long the recipe actually boils for, counting only the boil proper.
///
/// Ported from `Boil::boilTime_mins`.
pub fn boil_time_mins(recipe: &Recipe) -> f64 {
    let steps = recipe.boil_steps();
    step_phases(steps)
        .into_iter()
        .zip(steps)
        .filter(|(phase, _)| *phase == StepPhase::BoilProper)
        .filter_map(|(_, step)| step.step_time_mins)
        .sum()
}

/// Original and final gravity, each both as brewed and counting only fermentable sugar.
///
/// Ported from `recalcOgFg`.  Four numbers come out because the alcohol calculation needs the
/// gravities the yeast actually sees: a recipe with lactose in it finishes higher than the yeast
/// is responsible for, and crediting that to the alcohol would overstate it.
///
/// `mash_attenuation_multiplier` is what the mash model says this schedule does to attenuation;
/// `None` means there is no mash to judge.
pub fn gravities(
    recipe: &Recipe,
    final_volume_no_losses_l: f64,
    wort_from_mash_l: f64,
    mash_attenuation_multiplier: Option<f64>,
) -> (f64, f64, f64, f64) {
    let sugars = sugar_totals(recipe);
    let mut sugar_kg = sugars.sugars_from_starch_all_kg;
    let mut sugar_kg_ignore_efficiency = sugars.existing_sugars_all_kg;
    let mut non_fermentable_sugars_kg = sugars.existing_sugars_non_fermentable_kg;

    // Sugar that was already sugar is added to the kettle, so it is only diluted by what the
    // kettle loses -- not by mash efficiency.
    if let (Some(equipment), Some(_)) = (recipe.equipment.as_ref(), recipe.boil.as_ref()) {
        let kettle_wort_l = (wort_from_mash_l - lautering_deadspace_loss_l(equipment))
            + equipment.top_up_kettle.unwrap_or(DEFAULT_TOP_UP_KETTLE_L);
        let post_boil_wort_l = wort_end_of_boil_l(equipment, kettle_wort_l, boil_time_mins(recipe));
        let mut ratio = (post_boil_wort_l - equipment.kettle_trub_chiller_loss_l.unwrap_or(0.0)) / post_boil_wort_l;
        // Usually out of range because there is no mash yet, so assume nothing is lost.
        if ratio > 1.0 || ratio.is_nan() {
            ratio = 1.0;
        } else if ratio < 0.0 {
            ratio = 0.0;
        }
        sugar_kg_ignore_efficiency *= ratio;
        if non_fermentable_sugars_kg != 0.0 {
            non_fermentable_sugars_kg *= ratio;
        }
    }

    let efficiency_pct = recipe.row.efficiency.unwrap_or(0.0);
    sugar_kg = sugar_kg * efficiency_pct / 100.0 + sugar_kg_ignore_efficiency;
    non_fermentable_sugars_kg += sugars.sugars_from_starch_non_fermentable_kg * efficiency_pct / 100.0;

    let plato = algorithms::plato_from_sugar_mass_and_wort_volume(sugar_kg, final_volume_no_losses_l);
    let og = algorithms::plato_to_sg_20c20c(plato);
    let mut points = (og - 1.0) * 1000.0;

    let (og_fermentable, non_fermentable_points) = if non_fermentable_sugars_kg != 0.0 {
        let fermentable_kg = sugar_kg - non_fermentable_sugars_kg;
        let fermentable_plato =
            algorithms::plato_from_sugar_mass_and_wort_volume(fermentable_kg, final_volume_no_losses_l);
        let non_fermentable_plato =
            algorithms::plato_from_sugar_mass_and_wort_volume(non_fermentable_sugars_kg, final_volume_no_losses_l);
        (
            algorithms::plato_to_sg_20c20c(fermentable_plato),
            (algorithms::plato_to_sg_20c20c(non_fermentable_plato) - 1.0) * 1000.0,
        )
    } else {
        (og, 0.0)
    };

    let mut attenuation = attenuation_pct(recipe);
    if attenuation > 0.0
        && let Some(multiplier) = mash_attenuation_multiplier
    {
        attenuation = crate::mash::adjusted_attenuation_pct(attenuation, multiplier);
    }

    let (fg, fg_fermentable) = if non_fermentable_sugars_kg != 0.0 {
        let fermentable_points = (points - non_fermentable_points) * (1.0 - attenuation / 100.0);
        points = fermentable_points + non_fermentable_points;
        (1.0 + points / 1000.0, 1.0 + fermentable_points / 1000.0)
    } else {
        points *= 1.0 - attenuation / 100.0;
        let fg = 1.0 + points / 1000.0;
        (fg, fg)
    };

    (og, fg, og_fermentable, fg_fermentable)
}

/// Calories per litre.
///
/// Ported from `recalcCalories`, which works in Plato and never reports a negative figure.
pub fn calories_per_liter(og: f64, fg: f64) -> f64 {
    let plato = Unit::by_name("plato").expect("the Plato unit");
    let start_plato = plato.from_canonical(og);
    let finish_plato = plato.from_canonical(fg);
    let real_extract = (0.1808 * start_plato) + (0.8192 * finish_plato);
    let abw = (start_plato - real_extract) / (2.0665 - (0.010665 * start_plato));
    (((6.9 * abw) + 4.0 * (real_extract - 0.1)) * fg * 10.0).max(0.0)
}

/// Run the whole calculation, in the order the C++ runs it.
///
/// Ported from `Recipe::recalcAll`.  The order is a dependency order: grains feed the volumes,
/// the volumes feed the colour and the gravities, and the gravities feed the alcohol and the
/// calories.
///
/// `mash_attenuation_multiplier` comes from [`crate::mash`]; pass `None` to leave attenuation as
/// the yeast describes it.
pub fn calculate(recipe: &Recipe, formula: ColorFormula, mash_attenuation_multiplier: Option<f64>) -> Calculated {
    let (grains_kg, grains_in_mash_kg) = grains(recipe);
    let (wort_from_mash_l, boil_volume_l, post_boil_volume_l, final_volume_l) = volumes(recipe, grains_in_mash_kg);

    // The C++ works colour and gravity out against the batch size rather than the final volume,
    // because the losses after the kettle do not dilute anything -- they just mean less beer.
    let final_volume_no_losses_l = recipe.row.batch_size.unwrap_or(final_volume_l);

    let (color_mcu, color_srm) = color(recipe, final_volume_no_losses_l, formula);
    let (og, fg, og_fermentable, fg_fermentable) =
        gravities(recipe, final_volume_no_losses_l, wort_from_mash_l, mash_attenuation_multiplier);

    // Alcohol is worked out from the gravities the yeast actually sees, which is why those are
    // tracked separately from the ones a hydrometer would read.
    let abv_pct = if og_fermentable >= fg_fermentable {
        algorithms::abv_from_og_and_fg(og_fermentable, fg_fermentable)
    } else {
        0.0
    };

    Calculated {
        grains_kg,
        grains_in_mash_kg,
        wort_from_mash_l,
        boil_volume_l,
        post_boil_volume_l,
        final_volume_l,
        color_mcu,
        color_srm,
        og,
        fg,
        og_fermentable,
        fg_fermentable,
        abv_pct,
        calories_per_liter: calories_per_liter(og, fg),
    }
}
