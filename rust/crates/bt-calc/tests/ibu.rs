// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/ibu.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Mirrors `RecipeCalcTests::testIbuFormulas`, which is self-contained enough to port now: it
//! exercises the formulae directly rather than through a recipe.
//!
//! Same hop addition throughout: 56.7 g at 10% alpha acid into 19.87 litres at 1.050.

use bt_calc::ibu::*;

fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

/// Milligrams of alpha acid per litre, which every utilisation percentage multiplies.
const MG_PER_LITER: f64 = 0.10 * 56.7 * 1000.0 / 19.87;

fn parameters(time_in_boil_minutes: f64) -> Parameters {
    Parameters {
        alpha_acid_rating: 0.10,
        hops_grams: 56.7,
        post_boil_volume_liters: 19.87,
        wort_gravity_sg: 1.050,
        time_in_boil_minutes,
        hop_form: HopForm::Whole,
        original_gravity_sg: Some(1.050),
        ..Default::default()
    }
}

#[test]
fn which_formulae_model_what() {
    for formula in [Formula::Tinseth, Formula::Rager, Formula::Noonan, Formula::Garetz, Formula::Daniels] {
        assert!(!models_post_boil_isomerization(formula), "{formula:?}");
    }
    assert!(models_post_boil_isomerization(Formula::MIbu));
    assert!(models_post_boil_isomerization(Formula::Smph));

    assert!(uses_hopping_rate_factor(Formula::Garetz));
    assert!(uses_hopping_rate_factor(Formula::Daniels));
    assert!(!uses_hopping_rate_factor(Formula::Smph));
    assert!(!uses_hopping_rate_factor(Formula::Tinseth));
}

#[test]
fn garetz_follows_its_published_table() {
    for (minutes, utilization_pct) in [
        (5.0, 0.0),
        (10.0, 0.0),
        (12.0, 2.0),
        (20.0, 5.0),
        (30.0, 11.0),
        (45.0, 18.0),
        (60.0, 20.0),
        (75.0, 22.0),
        (90.0, 23.0),
        (120.0, 23.0),
    ] {
        let actual = ibus(&parameters(minutes), Formula::Garetz);
        assert!(close(actual, MG_PER_LITER * utilization_pct / 100.0, 0.001), "Garetz at {minutes} minutes: {actual}");
    }
}

#[test]
fn garetz_gives_pellets_a_bonus_only_in_the_first_half_hour() {
    let mut early = parameters(20.0);
    early.hop_form = HopForm::Pellet;
    assert!(close(ibus(&early, Formula::Garetz), MG_PER_LITER * 0.05 * 1.10, 0.001));

    // Past thirty minutes the extraction has caught up, so there is no bonus.
    let mut late = parameters(60.0);
    late.hop_form = HopForm::Pellet;
    assert!(close(ibus(&late, Formula::Garetz), MG_PER_LITER * 0.20, 0.001));
}

/// Garetz works from the gravity in the kettle, which is the original gravity scaled by how much
/// the boil concentrates the wort.  23.66 litres boiling down to 19.87 is a concentration factor
/// of 0.8398, so an OG of 1.060 is only 1.05039 in the kettle and barely needs correcting.
#[test]
fn garetz_corrects_for_gravity_in_the_kettle() {
    let mut strong = parameters(60.0);
    strong.original_gravity_sg = Some(1.060);
    strong.pre_boil_volume_liters = Some(23.66);
    assert!(close(ibus(&strong, Formula::Garetz), MG_PER_LITER * 0.20 / 1.0019442, 0.001));
}

#[test]
fn the_garetz_hopping_rate_factor_divides_everything() {
    let mut heavily_hopped = parameters(60.0);
    heavily_hopped.hopping_rate_factor = 1.25;
    assert!(close(ibus(&heavily_hopped, Formula::Garetz), MG_PER_LITER * 0.20 / 1.25, 0.001));
}

#[test]
fn a_post_boil_addition_contributes_nothing_to_the_boil_only_methods() {
    let mut after_flameout = parameters(0.0);
    after_flameout.post_boil_addition = true;
    for formula in [Formula::Garetz, Formula::Daniels, Formula::Tinseth, Formula::Rager, Formula::Noonan] {
        assert!(close(ibus(&after_flameout, formula), 0.0, 0.0001), "{formula:?}");
    }
}

/// The hopping rate factor is defined in terms of the bitterness it is used to compute, so the
/// test is that the answer satisfies its own equation.
#[test]
fn the_garetz_hopping_rate_factor_solves_its_own_equation() {
    for (total_ibus, concentration_factor) in [(57.071, 1.0), (120.0, 0.84), (0.0, 1.0)] {
        let factor = garetz_hopping_rate_factor(total_ibus, concentration_factor);
        assert!(
            close(factor, 1.0 + concentration_factor * (total_ibus / factor) / 260.0, 0.000001),
            "{total_ibus} at {concentration_factor}: {factor}"
        );
    }
    assert!(close(garetz_hopping_rate_factor(57.071, 1.0), 1.185203, 0.00001));
}

#[test]
fn daniels_follows_its_published_table_for_both_hop_forms() {
    for (minutes, whole_pct, pellet_pct) in [
        (0.0, 5.0, 6.0),
        (9.9, 5.0, 6.0),
        (10.0, 12.0, 15.0),
        (25.0, 15.0, 19.0),
        (30.0, 19.0, 24.0),
        (50.0, 22.0, 27.0),
        (60.0, 24.0, 30.0),
        (90.0, 27.0, 34.0),
    ] {
        let whole = parameters(minutes);
        let actual = ibus(&whole, Formula::Daniels);
        assert!(close(actual, MG_PER_LITER * whole_pct / 100.0, 0.001), "Daniels whole at {minutes}: {actual}");

        let mut pellet = parameters(minutes);
        pellet.hop_form = HopForm::Pellet;
        let actual = ibus(&pellet, Formula::Daniels);
        assert!(close(actual, MG_PER_LITER * pellet_pct / 100.0, 0.001), "Daniels pellet at {minutes}: {actual}");
    }
}

#[test]
fn daniels_corrects_for_gravity() {
    let mut strong = parameters(60.0);
    strong.wort_gravity_sg = 1.060;
    assert!(close(ibus(&strong, Formula::Daniels), MG_PER_LITER * 0.24 / 1.05, 0.001));
}

/// Tinseth does not model hop form itself, so the generic adjustment is applied on top.
#[test]
fn tinseth_takes_the_generic_hop_form_adjustment() {
    let whole = ibus(&parameters(60.0), Formula::Tinseth);

    let mut pellet = parameters(60.0);
    pellet.hop_form = HopForm::Pellet;
    assert!(close(ibus(&pellet, Formula::Tinseth), whole * 1.10, 0.0001));

    let mut plug = parameters(60.0);
    plug.hop_form = HopForm::Plug;
    assert!(close(ibus(&plug, Formula::Tinseth), whole * 1.02, 0.0001));
}
