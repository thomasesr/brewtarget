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
