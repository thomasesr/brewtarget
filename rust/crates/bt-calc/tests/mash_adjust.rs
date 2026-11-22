// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/mash_adjust.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The mash heat model, against the figures `MashAutoAdjusterTests.cpp` works out by hand.
//!
//! The C++ test writes its arithmetic into the comments -- "Heat needed = 2 × (66 - 20)
//! [grain] + 2 × (66 - 20) [tun] = 184 kcal, Water temp = 66 + 184 / 16 = 77.5 °C" -- so these
//! check the same numbers rather than a figure this port happened to produce.

use bt_calc::mash_adjust::{Contents, Mashing, Part};

/// `tolerance` in the C++ tests.
const TOLERANCE: f64 = 0.001;

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < TOLERANCE
}

/// `makeInputs` in `MashAutoAdjusterTests.cpp`.
fn mashing() -> Mashing {
    Mashing {
        grain_kg: 5.0,
        grain_temp_c: 20.0,
        tun_heat_capacity_kcal_c: 2.0,
        tun_temp_c: 20.0,
        boiling_point_c: 100.0,
    }
}

/// From `testStrikeWater`, whose comment does the arithmetic out loud.
///
/// 5 kg of grain at 0.4 kcal/kg/°C is a heat capacity of 2; the tun is another 2.  Bringing
/// both from 20 °C to 66 °C takes 2 × 46 + 2 × 46 = 184 kcal, and 16 L of water carries that
/// in 184 / 16 = 11.5 °C above the target.
#[test]
fn strike_water_is_as_hot_as_the_grain_and_the_tun_make_it() {
    let mashing = mashing();
    let strike = mashing.strike_water_temp_c(66.0, 16.0).expect("16 litres of strike water");
    assert!(close(strike, 77.5), "{strike}");

    // ...and the same mixing the other way round: that water at that temperature lands at 66.
    let settled = mashing.before_mashing_in().final_temp_c(16.0, strike);
    assert!(close(settled, 66.0), "{settled}");
}

/// `infusionBalances` in the C++ tests: the heat the water brings is the heat the mash takes.
#[test]
fn the_heat_the_water_brings_is_the_heat_the_mash_takes() {
    let contents = Contents {
        parts: vec![Part { heat_capacity_kcal_c: 2.0, temp_c: 20.0 }, Part { heat_capacity_kcal_c: 2.0, temp_c: 20.0 }],
    };
    assert!(close(contents.heat_capacity_kcal_c(), 4.0));
    assert!(close(contents.heat_needed_kcal(66.0), 184.0));
    // Cooling takes negative heat, which is the same sum the other way.
    assert!(close(contents.heat_needed_kcal(10.0), -40.0));
}

/// Asking the question the other way round: how much water, rather than how hot.
#[test]
fn the_amount_of_water_follows_from_how_hot_it_is() {
    let mashing = mashing();
    // Boiling water into the same mash: 184 kcal at 34 °C above the target is 5.411... litres.
    let amount = mashing.strike_water_amount_l(66.0, 100.0).expect("boiling water will do it");
    assert!(close(amount, 184.0 / 34.0), "{amount}");
    // ...and that amount at that temperature lands where it was asked to.
    assert!(close(mashing.before_mashing_in().final_temp_c(amount, 100.0), 66.0));
}

/// Water no hotter than the mash cannot heat it, however much of it there is.
#[test]
fn water_that_cannot_do_it_is_refused_rather_than_guessed_at() {
    let mashing = mashing();
    assert_eq!(mashing.strike_water_amount_l(66.0, 66.0), None, "water at the target temperature");
    assert_eq!(mashing.strike_water_amount_l(66.0, 20.0), None, "water colder than the grain");
    // Nothing to heat is no water at all rather than a failure.
    let settled = Mashing { grain_kg: 0.0, tun_heat_capacity_kcal_c: 0.0, ..mashing };
    assert_eq!(settled.strike_water_amount_l(20.0, 100.0), Some(0.0));
    // And no water means no temperature for it to be at.
    assert_eq!(mashing.strike_water_temp_c(66.0, 0.0), None);
}

/// Once the mash is under way everything in the tun is at one temperature, so a later infusion
/// mixes with a single body rather than with grain and tun separately.
#[test]
fn a_later_infusion_mixes_with_what_is_already_there() {
    let mashing = mashing();
    // 16 L in the tun, all settled at 66 °C: grain 2 + water 16 + tun 2 = 20 kcal/°C.
    let contents = mashing.during_mash(16.0, 66.0);
    assert!(close(contents.heat_capacity_kcal_c(), 20.0), "{}", contents.heat_capacity_kcal_c());

    // Raising that to 72 °C takes 20 × 6 = 120 kcal; boiling water carries 28 °C above the
    // target, so 120 / 28 litres of it.
    let amount = contents.amount_for(72.0, 100.0).expect("boiling water will do it");
    assert!(close(amount, 120.0 / 28.0), "{amount}");
    assert!(close(contents.final_temp_c(amount, 100.0), 72.0));
}

/// A mash has to happen between freezing and boiling, which the C++ checks before working
/// anything out rather than producing a schedule nobody can brew.
#[test]
fn a_temperature_outside_water_is_refused() {
    let mashing = mashing();
    assert!(mashing.temperature_makes_sense(66.0));
    assert!(!mashing.temperature_makes_sense(0.0), "freezing");
    assert!(!mashing.temperature_makes_sense(-5.0));
    assert!(!mashing.temperature_makes_sense(100.0), "boiling");
    assert!(!mashing.temperature_makes_sense(120.0));

    // At altitude the boiling point is lower, and so is what counts as possible.
    let altitude = Mashing { boiling_point_c: 93.0, ..mashing };
    assert!(altitude.temperature_makes_sense(90.0));
    assert!(!altitude.temperature_makes_sense(95.0));
}

// ---------------------------------------------------------------------------------------
// The scheduler, against the figures `MashAutoAdjusterTests.cpp` works out in its comments.
// ---------------------------------------------------------------------------------------

use bt_calc::mash_adjust::{
    DEFAULT_THICKNESS_L_KG, Inputs, Options, Severity, SpargeMethod, Step, calculate, set_sparge,
};
use bt_model::enums::MashStepType;

/// `makeInputs` in the C++ tests.
fn inputs() -> Inputs {
    Inputs {
        mashing: mashing(),
        grain_absorption_l_kg: 1.0,
        strike_water_allowance_l: 1.0,
        target_total_water_l: 30.0,
        mash_vessel_volume_l: Some(40.0),
        target_collected_wort_l: None,
    }
}

/// `thickness(x)` in the C++ tests.
fn thickness(l_kg: f64) -> Options {
    Options { thickness_l_kg: Some(l_kg), ..Options::default() }
}

fn saccharification() -> Step {
    Step::new("Saccharification", MashStepType::Infusion, 66.0)
}

fn warnings(outcome: &bt_calc::mash_adjust::Outcome) -> usize {
    outcome.problems.iter().filter(|problem| problem.severity == Severity::Warning).count()
}

/// `testStrikeWater`, whose comment reads:
/// "Strike water = 3 L/kg × 5 kg + 1 L allowance = 16 L; Heat needed = 184 kcal;
///  Water temp = 66 + 184 / 16 = 77.5 °C"
#[test]
fn the_strike_water_is_the_thickness_asked_for_at_the_temperature_that_reaches_the_rest() {
    let mut steps = vec![saccharification()];
    set_sparge(&mut steps, SpargeMethod::Batch, 1, 76.0);
    assert_eq!(steps.len(), 2, "a mash step and a batch sparge");

    let outcome = calculate(&inputs(), &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert_eq!(warnings(&outcome), 0, "{:?}", outcome.problems);

    let strike = &outcome.steps[0];
    assert!(close(strike.amount_l, 16.0), "{}", strike.amount_l);
    assert!(close(strike.infuse_temp_c.expect("a strike temperature"), 77.5));
    // The thickness it worked out to is recorded on the step.
    assert!(close(strike.liquor_to_grist_ratio_l_kg.expect("a ratio"), 3.0));
}

/// Same test: the step's own ratio is used when no thickness was asked for, then the thickness
/// the existing strike water implies, and only then the default.
#[test]
fn the_thickness_falls_back_through_what_the_mash_already_says() {
    let mut steps = vec![saccharification()];
    set_sparge(&mut steps, SpargeMethod::Batch, 1, 76.0);

    steps[0].liquor_to_grist_ratio_l_kg = Some(2.5);
    let outcome = calculate(&inputs(), &steps, &Options::default());
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].amount_l, 13.5), "{}", outcome.steps[0].amount_l);

    steps[0].liquor_to_grist_ratio_l_kg = None;
    steps[0].amount_l = 11.0; // 2 L/kg × 5 kg + 1 L
    let outcome = calculate(&inputs(), &steps, &Options::default());
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].amount_l, 11.0), "{}", outcome.steps[0].amount_l);

    steps[0].amount_l = 0.0;
    let outcome = calculate(&inputs(), &steps, &Options::default());
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].amount_l, DEFAULT_THICKNESS_L_KG * 5.0 + 1.0), "{}", outcome.steps[0].amount_l);
}

/// Same test: "with the tun pre-heated to 66 °C, it doesn't need any heat, so
/// water temp = 66 + 92 / 16 = 71.75 °C".
#[test]
fn a_preheated_tun_needs_cooler_strike_water() {
    let mut steps = vec![saccharification()];
    set_sparge(&mut steps, SpargeMethod::Batch, 1, 76.0);

    let mut inputs = inputs();
    inputs.mashing.tun_temp_c = 66.0;
    let outcome = calculate(&inputs, &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].infuse_temp_c.expect("a strike temperature"), 71.75));
}

/// `testFixedValues`: "Amount fixed at 20 L (so thickness ignored): water temp =
/// 66 + 184 / 20 = 75.2 °C", and "Sparge gets the rest of the water".
#[test]
fn a_pinned_amount_decides_the_temperature_and_the_sparge_takes_what_is_left() {
    let mut steps = vec![saccharification()];
    steps[0].amount_l = 20.0;
    steps[0].amount_fixed = true;
    set_sparge(&mut steps, SpargeMethod::Fly, 1, 76.0);

    let outcome = calculate(&inputs(), &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert_eq!(outcome.steps[0].amount_l, 20.0, "a pinned amount is not moved");
    assert!(close(outcome.steps[0].infuse_temp_c.expect("a strike temperature"), 75.2));
    // 30 L in total, 20 of them in the mash.
    assert!(close(outcome.steps[1].amount_l, 10.0), "{}", outcome.steps[1].amount_l);
}

/// Same test: "Temp fixed at 74 °C: amount = 184 / (74 - 66) = 23 L".
#[test]
fn a_pinned_temperature_decides_the_amount() {
    let mut steps = vec![saccharification()];
    steps[0].infuse_temp_c = Some(74.0);
    steps[0].infuse_temp_fixed = true;
    set_sparge(&mut steps, SpargeMethod::Fly, 1, 76.0);

    let outcome = calculate(&inputs(), &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].amount_l, 23.0), "{}", outcome.steps[0].amount_l);
}

/// `testNoSparge`: with no sparge, the mash water has to be all the water.
///
/// "Single infusion: all 30 L of water goes in as strike water, regardless of thickness.
///  Water temp = 66 + 184 / 30"
#[test]
fn with_no_sparge_a_single_infusion_carries_all_the_water() {
    let steps = vec![saccharification()];
    let outcome = calculate(&inputs(), &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].amount_l, 30.0), "{}", outcome.steps[0].amount_l);
    assert!(close(outcome.steps[0].infuse_temp_c.expect("a temperature"), 66.0 + 184.0 / 30.0));
}

/// Same test: "the strike water comes from the thickness (16 L) and the second infusion makes
/// up the rest (14 L).  Its temperature = 66 + 20 × (66 - 50) / 14".
#[test]
fn with_no_sparge_the_last_unpinned_infusion_makes_up_the_rest() {
    let steps = vec![Step::new("Protein rest", MashStepType::Infusion, 50.0), saccharification()];
    let outcome = calculate(&inputs(), &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    assert!(close(outcome.steps[0].amount_l, 16.0), "{}", outcome.steps[0].amount_l);
    assert!(close(outcome.steps[1].amount_l, 14.0), "{}", outcome.steps[1].amount_l);
    // The tun, grain and 16 L of water are all at 50 °C: heat capacity 2 + 16 + 2 = 20.
    assert!(close(outcome.steps[1].infuse_temp_c.expect("a temperature"), 66.0 + 20.0 * 16.0 / 14.0));
}

/// A mash with no grain in it cannot be worked out, and says so rather than dividing by it.
#[test]
fn a_mash_with_nothing_in_it_is_refused() {
    let mut without_grain = inputs();
    without_grain.mashing.grain_kg = 0.0;
    let outcome = calculate(&without_grain, &[saccharification()], &Options::default());
    assert!(!outcome.ok());
    assert!(outcome.problems[0].message.contains("no grains"), "{:?}", outcome.problems);

    let outcome = calculate(&inputs(), &[], &Options::default());
    assert!(!outcome.ok());
    assert!(outcome.problems[0].message.contains("no mash steps"), "{:?}", outcome.problems);
}

/// A step asking for more heat than boiling water can give is refused, with advice.
///
/// Which advice depends on what the brewer can actually change.  Thinning the mash only helps
/// where the strike water amount is the adjuster's to choose -- with no sparge it is not,
/// because that step has to make the water up to the total.
#[test]
fn a_step_that_cannot_be_reached_says_why_and_what_to_try() {
    // With a sparge, the strike water is the adjuster's to choose, so a thinner mash would help.
    let mut steps = vec![Step::new("Too hot", MashStepType::Infusion, 95.0)];
    set_sparge(&mut steps, SpargeMethod::Fly, 1, 76.0);
    let outcome = calculate(&inputs(), &steps, &thickness(1.0));
    assert!(!outcome.ok(), "{:?}", outcome.problems);
    let message = &outcome.problems[0].message;
    assert!(message.contains("above boiling point"), "{message}");
    assert!(message.contains("thinner mash"), "{message}");

    // Without one, that step has to carry all the water, so the advice is the other one.
    let outcome = calculate(&inputs(), &[Step::new("Too hot", MashStepType::Infusion, 95.0)], &thickness(1.0));
    assert!(!outcome.ok(), "{:?}", outcome.problems);
    let message = &outcome.problems[0].message;
    assert!(message.contains("above boiling point"), "{message}");
    assert!(message.contains("lower step temperature"), "{message}");
}

/// `set_sparge` leaves a mash alone when it already has the sparge asked for.
#[test]
fn setting_the_sparge_twice_changes_nothing_the_second_time() {
    let mut steps = vec![saccharification()];
    set_sparge(&mut steps, SpargeMethod::Batch, 2, 76.0);
    assert_eq!(steps.len(), 3, "one mash step and two batches");
    assert_eq!(steps[1].name, "Batch Sparge 1");
    assert_eq!(steps[2].name, "Batch Sparge 2");

    let before = steps.clone();
    set_sparge(&mut steps, SpargeMethod::Batch, 2, 76.0);
    assert_eq!(steps, before, "asking for what is already there should change nothing");

    // Changing method replaces them.
    set_sparge(&mut steps, SpargeMethod::Fly, 1, 76.0);
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[1].step_type, MashStepType::FlySparge);

    set_sparge(&mut steps, SpargeMethod::None, 0, 76.0);
    assert_eq!(steps.len(), 1, "no sparge leaves only the mash");
}

/// A decoction takes part of the mash, boils it and returns it; it cannot cool the mash.
#[test]
fn a_decoction_raises_the_mash_and_cannot_lower_it() {
    let steps = vec![
        Step::new("Protein rest", MashStepType::Infusion, 50.0),
        Step::new("Saccharification", MashStepType::Decoction, 66.0),
    ];
    let outcome = calculate(&inputs(), &steps, &thickness(3.0));
    assert!(outcome.ok(), "{:?}", outcome.problems);
    // Some of the mash, but not all of it.
    let taken = outcome.steps[1].amount_l;
    let mash_volume_l = outcome.steps[0].mash_volume_l.expect("a mash volume");
    assert!(taken > 0.0 && taken < mash_volume_l, "{taken} of {mash_volume_l}");
    // A decoction adds no water, so the mash is no bigger afterwards.
    assert!(close(outcome.steps[1].mash_volume_l.expect("a mash volume"), mash_volume_l));

    let cooling = vec![
        Step::new("Saccharification", MashStepType::Infusion, 66.0),
        Step::new("Cool down", MashStepType::Decoction, 50.0),
    ];
    let outcome = calculate(&inputs(), &cooling, &thickness(3.0));
    assert!(!outcome.ok());
    assert!(outcome.problems[0].message.contains("cannot lower"), "{:?}", outcome.problems);
}
