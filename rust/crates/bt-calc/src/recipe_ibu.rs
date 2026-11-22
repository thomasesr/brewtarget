// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/recipe_ibu.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Bitterness for a whole recipe.
//!
//! Ported from `recalcIBU`, `ibuContext`, `ibuBreakdown` and `Recipe::boilAdditionTiming` in
//! `src/model/Recipe.cpp`.
//!
//! The formulae in [`crate::ibu`] work on one hop addition in isolation; the work here is
//! deciding what to tell them about each one.  A hop added to the third of five boil steps is in
//! the kettle for a different length of time, at a different gravity, in a different volume, than
//! one added at the start -- and under SMPH, with however much alpha acid the earlier additions
//! already put into solution.

use bt_model::enums::{HopForm as ModelHopForm, RecipeAdditionStage};
use bt_model::recipe::{IngredientAmount, Recipe};
use bt_model::tables::{BoilStepRow, HopInRecipeRow, HopRow};

use crate::algorithms;
use crate::ibu::{self, Breakdown, Formula, HopForm, Parameters};
use crate::recipe::{StepPhase, boil_time_mins, step_phases};
use crate::recipe::{equiv_sucrose_kg, is_existing_sugar_type};
use crate::smph::{self, SmphParameters};

/// `Boil::default_boilTime_mins`.
const DEFAULT_BOIL_TIME_MINS: f64 = 60.0;

/// How long the recipe boils for, assuming an hour when there is no boil recorded at all.
///
/// The C++ writes this as `boil ? boil->boilTime_mins() : Boil::default_boilTime_mins`, so the
/// default applies only when the recipe has no boil object -- a boil whose steps happen to add
/// up to nothing really does boil for no time.
fn boil_time_or_default(recipe: &Recipe) -> f64 {
    if recipe.boil.is_none() { DEFAULT_BOIL_TIME_MINS } else { boil_time_mins(recipe) }
}
/// `Equipment::default_hopUtilization_pct`, as a fraction.
const DEFAULT_HOP_UTILIZATION: f64 = 1.0;
/// The C++ reads these from the user's settings; the defaults are what it falls back on.
pub const DEFAULT_FIRST_WORT_HOP_ADJUSTMENT: f64 = 1.1;
pub const DEFAULT_MASH_HOP_ADJUSTMENT: f64 = 0.0;

/// When a boil addition goes in, relative to the boil proper.
///
/// Ported from `Recipe::BoilAdditionTiming`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BoilAdditionTiming {
    /// Whether the addition happens after the boil proper has finished.
    pub post_boil: bool,
    pub boil_proper_mins: f64,
    pub after_flameout_mins: f64,
    /// How long it stays in after that, where that is knowable.
    pub post_boil_remaining_mins: Option<f64>,
    pub until_end_of_step_mins: Option<f64>,
}

fn ramp_time_mins(step: &BoilStepRow) -> f64 {
    step.ramp_time_mins.unwrap_or(0.0).max(0.0)
}

/// Work out when a boil addition goes in.
///
/// Ported from `Recipe::boilAdditionTiming`.  Without a boil, or without a step recorded, the
/// addition is taken to be in the boil proper for however long it says -- which is how a recipe
/// imported from BeerXML, where additions carry only a time, behaves.
pub fn boil_addition_timing(
    recipe: &Recipe,
    add_at_time_mins: Option<f64>,
    step: Option<i32>,
    duration_mins: Option<f64>,
) -> BoilAdditionTiming {
    let boil_time = boil_time_or_default(recipe);
    let boil_proper_only =
        || BoilAdditionTiming { boil_proper_mins: add_at_time_mins.unwrap_or(boil_time), ..Default::default() };

    let steps = recipe.boil_steps();
    let Some(step) = step else { return boil_proper_only() };
    if recipe.boil.is_none() || step < 1 || step as usize > steps.len() {
        return boil_proper_only();
    }

    let step_index = step as usize - 1;
    let phases = step_phases(steps);
    let Some(last_boil_proper_index) =
        phases.iter().enumerate().rev().find(|(_, phase)| **phase == StepPhase::BoilProper).map(|(index, _)| index)
    else {
        return boil_proper_only(); // nothing here counts as boiling
    };

    let addition_step = &steps[step_index];
    let step_length_mins = addition_step.step_time_mins;

    if step_index > last_boil_proper_index {
        // The addition is after the boil: a whirlpool or a hop stand.
        let mut timing = BoilAdditionTiming { post_boil: true, ..Default::default() };
        let addition_step_ramp_time_mins = ramp_time_mins(addition_step);
        let mut mins_into_step = 0.0;

        if let Some(add_at_time_mins) = add_at_time_mins {
            timing.until_end_of_step_mins = Some(add_at_time_mins);
            if let Some(step_length_mins) = step_length_mins {
                mins_into_step = (step_length_mins - add_at_time_mins).max(-addition_step_ramp_time_mins);
            }
        } else {
            timing.until_end_of_step_mins = step_length_mins;
        }

        timing.after_flameout_mins = addition_step_ramp_time_mins + mins_into_step;
        for step in &steps[last_boil_proper_index + 1..step_index] {
            timing.after_flameout_mins += ramp_time_mins(step) + step.step_time_mins.unwrap_or(0.0);
        }

        // How much longer it stays in, which is unknowable if any later step has no length.
        timing.post_boil_remaining_mins = timing.until_end_of_step_mins;
        for step in &steps[step_index + 1..] {
            let Some(remaining) = timing.post_boil_remaining_mins else { break };
            timing.post_boil_remaining_mins =
                step.step_time_mins.map(|subsequent| remaining + ramp_time_mins(step) + subsequent);
        }
        if let Some(duration_mins) = duration_mins {
            timing.post_boil_remaining_mins =
                Some(timing.post_boil_remaining_mins.map_or(duration_mins, |remaining| remaining.min(duration_mins)));
        }
        return timing;
    }

    // The addition is in or before the boil proper, so it boils for the rest of its own step plus
    // every boiling step after it.
    let subsequent_boil_proper_mins: f64 = steps[step_index + 1..=last_boil_proper_index]
        .iter()
        .zip(&phases[step_index + 1..=last_boil_proper_index])
        .filter(|(_, phase)| **phase == StepPhase::BoilProper)
        .filter_map(|(step, _)| step.step_time_mins)
        .sum();

    BoilAdditionTiming {
        boil_proper_mins: if phases[step_index] == StepPhase::BoilProper {
            add_at_time_mins.unwrap_or(step_length_mins.unwrap_or(0.0)) + subsequent_boil_proper_mins
        } else {
            subsequent_boil_proper_mins
        },
        ..Default::default()
    }
}

/// Sugar in the kettle, and when each lot of it arrived.
///
/// Ported from `Recipe::impl::KettleSugars`.  A hop added early boils in thinner wort than one
/// added after the late malt extract goes in, and under every method bitterness extraction falls
/// as gravity rises, so when the sugar arrives matters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KettleSugars {
    pub at_start_of_boil_kg: f64,
    /// Sugar added during the boil: how long before flameout, and how much.
    pub in_boil_additions: Vec<(f64, f64)>,
}

impl KettleSugars {
    /// How much sugar is in the kettle at a point in the boil.
    pub fn in_kettle_kg(&self, minutes_before_flameout: f64) -> f64 {
        self.at_start_of_boil_kg
            + self
                .in_boil_additions
                .iter()
                .filter(|(added_before_flameout_mins, _)| *added_before_flameout_mins >= minutes_before_flameout)
                .map(|(_, sugar_kg)| sugar_kg)
                .sum::<f64>()
    }
}

/// Work out what sugar is in the kettle and when.
///
/// Ported from `Recipe::impl::kettleSugars`.  Anything added at fermentation or packaging never
/// sees the kettle; anything added after the boil proper does not either.
pub fn kettle_sugars(recipe: &Recipe) -> KettleSugars {
    let boil_time = boil_time_or_default(recipe);
    let efficiency = recipe.row.efficiency.unwrap_or(0.0) / 100.0;
    let mut sugars = KettleSugars::default();

    for addition in &recipe.fermentables {
        let stage = addition.addition.stage().ok().flatten();
        if matches!(stage, Some(RecipeAdditionStage::Fermentation | RecipeAdditionStage::Packaging)) {
            continue;
        }
        let existing_sugar =
            is_existing_sugar_type(addition.ingredient.as_ref().and_then(|f| f.ftype().ok().flatten()));
        let sugar_kg = equiv_sucrose_kg(addition) * if existing_sugar { 1.0 } else { efficiency };

        if stage == Some(RecipeAdditionStage::Boil) {
            let timing = boil_addition_timing(
                recipe,
                addition.addition.add_at_time_mins,
                addition.addition.step,
                addition.addition.duration_mins,
            );
            if timing.post_boil {
                continue; // never boiled, so it does not thicken the wort the hops see
            }
            if timing.boil_proper_mins < boil_time - 0.01 {
                sugars.in_boil_additions.push((timing.boil_proper_mins.max(0.0), sugar_kg));
                continue;
            }
        }
        sugars.at_start_of_boil_kg += sugar_kg;
    }
    sugars
}

/// How much wort is in the kettle at a point in the boil.
///
/// Ported from `Recipe::impl::kettleVolume_l`: the kettle starts at the pre-boil volume and
/// evaporates linearly down to the flameout volume.
fn kettle_volume_l(parameters: &Parameters, boil_time_mins: f64, minutes_before_flameout: f64) -> f64 {
    let at_flameout_l = parameters.kettle_volume_at_flameout_liters.unwrap_or(parameters.post_boil_volume_liters);
    let Some(pre_boil_volume_liters) = parameters.pre_boil_volume_liters else {
        return at_flameout_l;
    };
    if minutes_before_flameout <= 0.0 || boil_time_mins <= 0.0 {
        return at_flameout_l;
    }
    let proportion_of_boil_remaining = (minutes_before_flameout / boil_time_mins).min(1.0);
    at_flameout_l + (pre_boil_volume_liters - at_flameout_l) * proportion_of_boil_remaining
}

/// The gravity in the kettle at a point in the boil.
fn kettle_gravity_sg(
    parameters: &Parameters,
    sugars: &KettleSugars,
    boil_time_mins: f64,
    minutes_before_flameout: f64,
) -> f64 {
    let minutes = minutes_before_flameout.max(0.0);
    let volume_l = kettle_volume_l(parameters, boil_time_mins, minutes);
    if volume_l <= 0.0 {
        return 1.0;
    }
    algorithms::plato_to_sg_20c20c(algorithms::plato_from_sugar_mass_and_wort_volume(
        sugars.in_kettle_kg(minutes),
        volume_l,
    ))
}

/// The gravity a hop addition sees averaged over its time in the kettle.
///
/// Ported from `averageKettleGravity_sg`.  The wort thickens as it boils, so a 60-minute
/// addition spends its first half-hour in noticeably thinner wort than its last; averaging over
/// tenth-of-a-minute steps is what the C++ does.
fn average_kettle_gravity_sg(
    parameters: &Parameters,
    sugars: &KettleSugars,
    boil_time_mins: f64,
    minutes_before_flameout: f64,
) -> f64 {
    if minutes_before_flameout <= 0.0 {
        return kettle_gravity_sg(parameters, sugars, boil_time_mins, 0.0);
    }
    let number_of_steps = ((minutes_before_flameout / 0.1).ceil() as i64).max(1);
    let step_size = minutes_before_flameout / number_of_steps as f64;
    let total: f64 = (0..number_of_steps)
        .map(|step| kettle_gravity_sg(parameters, sugars, boil_time_mins, (step as f64 + 0.5) * step_size))
        .sum();
    total / number_of_steps as f64
}

/// The settings the C++ reads from the user's preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IbuSettings {
    pub formula: Formula,
    /// First wort hops are credited with a little more bitterness than their time suggests.
    pub first_wort_hop_adjustment: f64,
    /// How much, if anything, to credit a mash hop with.  Zero by default: most authorities say
    /// mash hops contribute almost nothing.
    pub mash_hop_adjustment: f64,
}

impl Default for IbuSettings {
    fn default() -> Self {
        Self {
            formula: Formula::default(),
            first_wort_hop_adjustment: DEFAULT_FIRST_WORT_HOP_ADJUSTMENT,
            mash_hop_adjustment: DEFAULT_MASH_HOP_ADJUSTMENT,
        }
    }
}

/// What every hop addition in a recipe shares.
///
/// Ported from `Recipe::impl::IbuContext`.
#[derive(Debug, Clone, PartialEq)]
pub struct IbuContext {
    pub common: Parameters,
    pub smph: SmphParameters,
    pub sugars: KettleSugars,
    pub boil_time_mins: f64,
    pub hop_utilization: f64,
    /// Alpha acid already dissolved before each hop addition, keyed by position in
    /// `recipe.hops`.  Only SMPH needs it, since only SMPH models saturation.
    pub dissolved_before: Vec<f64>,
    pub kettle_volume_at_addition: Vec<f64>,
}

fn hop_form_of(hop: Option<&HopRow>) -> HopForm {
    match hop.and_then(|hop| hop.form().ok().flatten()) {
        Some(ModelHopForm::Leaf | ModelHopForm::WetLeaf) | None => HopForm::Whole,
        Some(ModelHopForm::Plug) => HopForm::Plug,
        Some(ModelHopForm::Pellet) => HopForm::Pellet,
        Some(ModelHopForm::Powder) => HopForm::Powder,
        Some(ModelHopForm::Extract) => HopForm::Extract,
    }
}

/// How long before flameout an addition goes in, which is negative for a post-boil one.
fn minutes_before_flameout(recipe: &Recipe, addition: &HopInRecipeRow) -> f64 {
    let timing = boil_addition_timing(recipe, addition.add_at_time_mins, addition.step, addition.duration_mins);
    if timing.post_boil { -timing.after_flameout_mins } else { timing.boil_proper_mins }
}

/// Build what all the hop additions share, including the saturation state SMPH needs.
///
/// Ported from `Recipe::impl::ibuContext`.
pub fn ibu_context(recipe: &Recipe, original_gravity_sg: f64, settings: IbuSettings) -> IbuContext {
    let boil_time = boil_time_or_default(recipe);
    let sugars = kettle_sugars(recipe);

    let mut common = Parameters {
        post_boil_volume_liters: recipe.row.batch_size.unwrap_or(0.0),
        time_in_boil_minutes: boil_time, // overridden per addition
        original_gravity_sg: Some(original_gravity_sg),
        ..Default::default()
    };
    let mut smph_parameters = SmphParameters::default();

    if let Some(equipment) = &recipe.equipment {
        common.kettle_internal_diameter_cm = equipment.kettle_internal_diameter_cm;
        common.kettle_opening_diameter_cm = equipment.kettle_opening_diameter_cm;
        smph_parameters.boiling_point_c = equipment.boiling_point;
    }
    if let Some((boil, _)) = &recipe.boil {
        common.pre_boil_volume_liters = boil.pre_boil_size_l;
    }

    // The pH the wort finishes the boil at, which is the last one any boiling step records.
    let phases = step_phases(recipe.boil_steps());
    for (step, phase) in recipe.boil_steps().iter().zip(&phases) {
        if *phase == StepPhase::BoilProper && step.end_acidity_ph.is_some() {
            smph_parameters.post_boil_ph = step.end_acidity_ph;
        }
    }

    common.wort_gravity_sg = average_kettle_gravity_sg(&common, &sugars, boil_time, boil_time);
    common.flameout_wort_gravity_sg = Some(kettle_gravity_sg(&common, &sugars, boil_time, 0.0));

    smph_parameters.yeast_flocculation =
        recipe.yeasts.iter().find_map(|addition| addition.ingredient.as_ref()?.flocculation().ok().flatten());
    smph_parameters.wort_clarity = recipe.row.wort_clarity().ok().flatten();
    if let Some((fermentation, _)) = &recipe.fermentation {
        smph_parameters.krausen_handling = fermentation.krausen_handling().ok().flatten();
    }

    // Under SMPH each addition dissolves into wort that already holds whatever the earlier ones
    // put there, so the additions have to be walked in the order they go in.
    let mut dissolved_before = vec![0.0; recipe.hops.len()];
    let mut kettle_volume_at_addition = vec![common.post_boil_volume_liters; recipe.hops.len()];
    for (index, addition) in recipe.hops.iter().enumerate() {
        let minutes = minutes_before_flameout(recipe, &addition.addition);
        kettle_volume_at_addition[index] = kettle_volume_l(&common, boil_time, minutes.max(0.0));
    }
    if settings.formula == Formula::Smph {
        let mut order: Vec<usize> = (0..recipe.hops.len()).collect();
        // Stable, and latest-first, exactly as the C++ sorts it.
        order.sort_by(|left, right| {
            minutes_before_flameout(recipe, &recipe.hops[*right].addition)
                .total_cmp(&minutes_before_flameout(recipe, &recipe.hops[*left].addition))
        });
        let mut dissolved_ppm = 0.0;
        for index in order {
            let addition = &recipe.hops[index];
            if addition.addition.stage().ok().flatten() != Some(RecipeAdditionStage::Boil) {
                continue;
            }
            let Some(amount) = addition.addition.canonical_amount() else { continue };
            let Some(hop) = &addition.ingredient else { continue };
            dissolved_before[index] = dissolved_ppm;
            let added_ppm = (hop.alpha_pct.unwrap_or(0.0) / 100.0) * amount.quantity * 1000.0 * 1000.0
                / kettle_volume_at_addition[index];
            dissolved_ppm = smph::dissolved_after_addition_ppm(dissolved_ppm, added_ppm);
        }
    }

    IbuContext {
        common,
        smph: smph_parameters,
        sugars,
        boil_time_mins: boil_time,
        hop_utilization: recipe
            .equipment
            .as_ref()
            .and_then(|equipment| equipment.hop_utilization)
            .map_or(DEFAULT_HOP_UTILIZATION, |pct| pct / 100.0),
        dissolved_before,
        kettle_volume_at_addition,
    }
}

/// Bitterness from one hop addition, in the context of the recipe it belongs to.
///
/// Ported from `Recipe::impl::ibuBreakdown`.  A first wort hop is credited with a little more
/// than its time suggests; a mash hop with whatever the user's setting says, which is nothing by
/// default.
pub fn addition_breakdown(recipe: &Recipe, index: usize, context: &IbuContext, settings: IbuSettings) -> Breakdown {
    let Some(addition) = recipe.hops.get(index) else {
        return Breakdown::default();
    };
    let Some(hop) = &addition.ingredient else {
        return Breakdown::default();
    };
    let Some(amount) = addition.addition.canonical_amount() else {
        return Breakdown::default();
    };

    let mut parameters = context.common.clone();
    parameters.alpha_acid_rating = hop.alpha_pct.unwrap_or(0.0) / 100.0;
    parameters.hops_grams = amount.quantity * 1000.0;
    parameters.hop_form = hop_form_of(Some(hop));

    let mut smph_parameters = context.smph.clone();
    smph_parameters.beta_acid_rating = hop.beta_pct.map(|pct| pct / 100.0);
    smph_parameters.six_month_alpha_loss = hop.six_month_alpha_loss_pct.map(|pct| pct / 100.0);
    smph_parameters.polyphenol_rating = hop.polyphenols_pct.map(|pct| pct / 100.0);

    let stage = addition.addition.stage().ok().flatten();
    let mut breakdown = match stage {
        Some(RecipeAdditionStage::Boil) => {
            // A first wort hop goes in before the boil starts, so it boils for the whole time.
            let first_wort = is_first_wort(recipe, &addition.addition);
            if !first_wort {
                let timing = boil_addition_timing(
                    recipe,
                    addition.addition.add_at_time_mins,
                    addition.addition.step,
                    addition.addition.duration_mins,
                );
                parameters.time_in_boil_minutes = timing.boil_proper_mins;
                parameters.wort_gravity_sg = average_kettle_gravity_sg(
                    &context.common,
                    &context.sugars,
                    context.boil_time_mins,
                    timing.boil_proper_mins,
                );
                if timing.post_boil {
                    parameters.post_boil_addition = true;
                    parameters.post_boil_delay_minutes = timing.after_flameout_mins;
                    parameters.cool_time_minutes = timing.post_boil_remaining_mins;
                }
            }
            smph_parameters.kettle_volume_at_addition_liters = context.kettle_volume_at_addition.get(index).copied();
            smph_parameters.kettle_dissolved_alpha_acids_ppm = context.dissolved_before.get(index).copied();

            let breakdown = ibu::ibu_breakdown_with(&parameters, settings.formula, &smph_parameters);
            if first_wort { scaled(breakdown, settings.first_wort_hop_adjustment) } else { breakdown }
        }
        Some(RecipeAdditionStage::Mash) if settings.mash_hop_adjustment > 0.0 => scaled(
            ibu::ibu_breakdown_with(&parameters, settings.formula, &smph_parameters),
            settings.mash_hop_adjustment,
        ),
        // A hop added at fermentation or packaging is a dry hop: aroma, not bitterness.
        _ => Breakdown::default(),
    };

    breakdown = scaled(breakdown, context.hop_utilization);
    breakdown
}

fn scaled(breakdown: Breakdown, factor: f64) -> Breakdown {
    Breakdown {
        boil_ibus: breakdown.boil_ibus * factor,
        post_boil_ibus: breakdown.post_boil_ibus * factor,
        non_iaa_ibus: breakdown.non_iaa_ibus * factor,
    }
}

/// Whether a hop goes into the first wort, before the boil has started.
///
/// Ported from `RecipeAdditionHop::isFirstWort`: an addition to the first boil step, timed at
/// the very start of it.
fn is_first_wort(recipe: &Recipe, addition: &HopInRecipeRow) -> bool {
    if addition.stage().ok().flatten() != Some(RecipeAdditionStage::Boil) {
        return false;
    }
    let first_step_is_pre_boil = step_phases(recipe.boil_steps()).first() == Some(&StepPhase::PreBoil);
    first_step_is_pre_boil && addition.step == Some(1) && addition.add_at_time_mins.unwrap_or(0.0) == 0.0
}

/// Total bitterness for the recipe, and the contribution of each hop addition.
///
/// Ported from `recalcIBU`.  Hops are not the only source: malt contributes polyphenols that
/// SMPH accounts for, and a fermentable may carry its own published bitterness figure.
pub fn recipe_ibus(recipe: &Recipe, original_gravity_sg: f64, settings: IbuSettings) -> (f64, Vec<f64>) {
    let context = ibu_context(recipe, original_gravity_sg, settings);

    let per_addition: Vec<f64> =
        (0..recipe.hops.len()).map(|index| addition_breakdown(recipe, index, &context, settings).total()).collect();
    let mut total: f64 = per_addition.iter().sum();

    if settings.formula == Formula::Smph {
        total += smph::recipe_level_ibus(original_gravity_sg, context.smph.post_boil_ph);
    }

    // Some fermentables -- roast barley, chocolate malt -- carry a published bitterness in IBU
    // per gallon per pound, which converts into the batch at 8.34538 pounds per gallon.
    let batch_size_l = recipe.row.batch_size.unwrap_or(0.0);
    if batch_size_l > 0.0 {
        for addition in &recipe.fermentables {
            let Some(fermentable) = &addition.ingredient else { continue };
            let Some(amount) = addition.addition.canonical_amount() else { continue };
            total += fermentable.ibu_gal_per_lb.unwrap_or(0.0) * (amount.quantity / batch_size_l) / 8.34538;
        }
    }
    (total, per_addition)
}
