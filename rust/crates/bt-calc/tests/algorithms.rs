// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/algorithms.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Checks on the ported calculations.
//!
//! The recipe-level tests in `src/unitTests/RecipeCalcTests.cpp` need the whole model -- recipes,
//! mashes, equipment -- so they wait for the hand-written model layer.  What is checked here is
//! the arithmetic underneath: known physical values, the published tables these formulae come
//! from, and the identities each one has to satisfy.

use bt_calc::algorithms::*;
use bt_calc::{ColorFormula, mcu_to_srm, srm_to_display_color};

fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

#[test]
fn rounding_matches_the_cpp() {
    assert_eq!(round(1.2345, 2), 1.23);
    assert_eq!(round(1.2355, 2), 1.24);
    assert_eq!(round(1234.5, -2), 1200.0);
    assert_eq!(round(1.0, 0), 1.0);
}

/// Water is densest a little above freezing, at almost exactly 1 kg/L, and noticeably lighter at
/// room temperature.  A polynomial transcribed with a digit out of place would not do that.
#[test]
fn water_density_follows_the_physical_curve() {
    assert!(close(water_density_kg_l(4.0), 1.00009, 0.0001), "{}", water_density_kg_l(4.0));
    assert!(close(water_density_kg_l(20.0), 0.99815, 0.0001), "{}", water_density_kg_l(20.0));
    assert!(water_density_kg_l(4.0) > water_density_kg_l(0.0));
    assert!(water_density_kg_l(4.0) > water_density_kg_l(30.0));
}

#[test]
fn a_hydrometer_needs_no_correction_at_its_calibration_temperature() {
    // Reading at the temperature it was calibrated for leaves the gravity untouched.
    assert!(close(correct_sg_for_temperature(1.050, 20.0, 20.0), 1.050, 1e-12));
    // Warmer wort is less dense, so the true gravity is higher than the reading.
    assert!(correct_sg_for_temperature(1.050, 30.0, 20.0) > 1.050);
    assert!(correct_sg_for_temperature(1.050, 10.0, 20.0) < 1.050);
}

/// The correction is a curve fitted to observations, not an exact formula, so it does not come
/// out at precisely zero at the 15 °C the hydrometer is calibrated for -- it is about 1e-4 there.
/// What matters is that it crosses zero near 15 °C and grows with temperature, since warmer wort
/// reads low.
#[test]
fn hydrometer_correction_crosses_zero_near_the_calibration_temperature() {
    let at_15 = hydrometer_15c_correction(15.0);
    assert!(at_15.abs() < 1e-3, "{at_15}");
    assert!(hydrometer_15c_correction(0.0) < 0.0);
    assert!(hydrometer_15c_correction(20.0) > at_15);
    assert!(hydrometer_15c_correction(30.0) > hydrometer_15c_correction(20.0));
}

/// The HMRC table: a drop from 1.050 to 1.010 is 40.0 degrees of excess gravity, which falls in
/// the 36.1 to 46.5 band with a factor of 0.130, giving 5.2% ABV.
#[test]
fn alcohol_by_volume_uses_the_published_table() {
    assert!(close(abv_from_og_and_fg(1.050, 1.010), 5.2, 1e-9), "{}", abv_from_og_and_fg(1.050, 1.010));
    // No fermentation, no alcohol.
    assert_eq!(abv_from_og_and_fg(1.040, 1.040), 0.0);
    // And it rises with the drop in gravity.
    assert!(abv_from_og_and_fg(1.060, 1.010) > abv_from_og_and_fg(1.050, 1.010));
}

#[test]
#[should_panic(expected = "final gravity")]
fn alcohol_by_volume_rejects_gravities_the_wrong_way_round() {
    abv_from_og_and_fg(1.010, 1.050);
}

#[test]
fn refractive_index_of_water_is_the_published_value() {
    assert!(close(refractive_index(0.0), 1.33302, 1e-9));
    assert!(refractive_index(10.0) > refractive_index(0.0), "sugar raises the refractive index");
}

#[test]
fn plato_from_sugar_and_wort() {
    // Zero sugar in zero wort is the Plato of water rather than a division by zero.
    assert_eq!(plato_from_sugar_mass_and_wort_volume(0.0, 0.0), 0.0);
    // One kilogram of sucrose in ten litres is a little over ten percent by mass.
    let plato = plato_from_sugar_mass_and_wort_volume(1.0, 10.0);
    assert!((9.0..11.0).contains(&plato), "{plato}");
    // More sugar in the same wort is a higher reading.
    assert!(plato_from_sugar_mass_and_wort_volume(2.0, 10.0) > plato);
}

#[test]
fn colour_approximations_agree_with_their_formulae() {
    assert!(close(mcu_to_srm(10.0, ColorFormula::Morey), 7.2398, 0.0001));
    assert!(close(mcu_to_srm(10.0, ColorFormula::Daniel), 10.4, 1e-9));
    assert!(close(mcu_to_srm(10.0, ColorFormula::Mosher), 7.7, 1e-9));
    assert_eq!(ColorFormula::default(), ColorFormula::Morey, "the C++ default");
    assert_eq!(ColorFormula::Morey.as_str(), "morey");
}

#[test]
fn display_colour_runs_from_pale_to_black() {
    // A very pale beer is capped at 253 red rather than overflowing.
    let pale = srm_to_display_color(0.0);
    assert_eq!(pale[0], 253);
    // Above 35 SRM the green channel is forced to zero, and blue has long since clamped there.
    // Red has not: at 40 SRM the formula still gives about 40, so a stout draws very dark red
    // rather than true black.
    assert_eq!(srm_to_display_color(40.0), [40, 0, 0]);
    // Colour darkens monotonically in between.
    assert!(srm_to_display_color(5.0)[2] > srm_to_display_color(10.0)[2]);
}

#[test]
fn gravity_and_plato_round_trip() {
    for sg in [1.010, 1.040, 1.060, 1.092] {
        let plato = sg_20c20c_to_plato(sg);
        assert!(close(plato_to_sg_20c20c(plato), sg, 1e-6), "{sg} -> {plato} -> {}", plato_to_sg_20c20c(plato));
    }
}
