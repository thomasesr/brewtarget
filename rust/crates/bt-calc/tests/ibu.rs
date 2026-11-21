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

// ── What happens after flameout ───────────────────────────────────────────────────────────────
//
// Mirrors `RecipeCalcTests::testPostBoilIbus`, which checks the mIBU implementation against
// Hosom's own worked examples.  Same hop addition as above, into 1.060 wort in a 38 cm kettle.

fn post_boil_parameters(time_in_boil_minutes: f64) -> Parameters {
    Parameters {
        alpha_acid_rating: 0.10,
        hops_grams: 56.7,
        post_boil_volume_liters: 19.87,
        wort_gravity_sg: 1.060,
        time_in_boil_minutes,
        kettle_internal_diameter_cm: Some(38.0),
        kettle_opening_diameter_cm: Some(38.0),
        ..Default::default()
    }
}

#[test]
fn tinseth_matches_hosoms_worked_example_and_ignores_the_cooling() {
    let mut parameters = post_boil_parameters(6.0);
    let tinseth_6 = ibus(&parameters, Formula::Tinseth);
    assert!(close(tinseth_6 / MG_PER_LITER, 0.0493, 0.0005), "utilization {}", tinseth_6 / MG_PER_LITER);

    // Tinseth knows nothing about what happens after the burner goes out.
    parameters.cool_time_minutes = Some(10.0);
    assert!(close(ibus(&parameters, Formula::Tinseth), tinseth_6, 0.0001));
}

#[test]
fn mibu_is_tinseth_when_there_is_no_cooling_to_model() {
    let tinseth_6 = ibus(&post_boil_parameters(6.0), Formula::Tinseth);

    // An unrecorded cool time is treated as none at all.
    let unknown = ibu_breakdown(&post_boil_parameters(6.0), Formula::MIbu);
    assert!(close(unknown.boil_ibus, tinseth_6, 0.0001), "boil {}", unknown.boil_ibus);
    assert!(close(unknown.post_boil_ibus, 0.0, 0.0001), "post-boil {}", unknown.post_boil_ibus);

    let mut zero = post_boil_parameters(6.0);
    zero.cool_time_minutes = Some(0.0);
    assert!(close(ibus(&zero, Formula::MIbu), tinseth_6, 0.0001));
}

/// Hosom's example: a six-minute addition with ten minutes of cooling gains rather more
/// bitterness while the wort sits than the boil-only methods would credit it with.
#[test]
fn mibu_adds_the_bitterness_gained_while_the_wort_cools() {
    let tinseth_6 = ibus(&post_boil_parameters(6.0), Formula::Tinseth);
    let mut parameters = post_boil_parameters(6.0);
    parameters.cool_time_minutes = Some(10.0);

    let breakdown = ibu_breakdown(&parameters, Formula::MIbu);
    assert!(close(breakdown.boil_ibus, tinseth_6, 0.0001), "boil {}", breakdown.boil_ibus);
    assert!(close(breakdown.post_boil_ibus, 12.355, 0.02), "post-boil {}", breakdown.post_boil_ibus);
    assert!(close(breakdown.total(), 26.0, 0.6), "total {}", breakdown.total());
}

/// A flameout addition contributes nothing at all to the boil-only methods, but is most of the
/// bitterness in a long hop stand.
#[test]
fn a_flameout_addition_only_counts_for_the_methods_that_model_cooling() {
    let mut parameters = post_boil_parameters(0.0);
    parameters.post_boil_addition = true;
    parameters.cool_time_minutes = Some(30.0);
    parameters.kettle_opening_diameter_cm = Some(25.0);

    let breakdown = ibu_breakdown(&parameters, Formula::MIbu);
    assert!(close(breakdown.boil_ibus, 0.0, 0.0001), "it was never in the boil");
    assert!(close(breakdown.post_boil_ibus, 27.742, 0.05), "post-boil {}", breakdown.post_boil_ibus);

    for formula in [Formula::Tinseth, Formula::Rager, Formula::Noonan] {
        assert!(!models_post_boil_isomerization(formula));
        assert!(close(ibus(&parameters, formula), 0.0, 0.0001), "{formula:?} should see nothing");
    }
    assert!(models_post_boil_isomerization(Formula::MIbu));
}

/// A hop added ten minutes after flameout, into wort that has already cooled a little, gets less
/// than one added at flameout would.
#[test]
fn a_delayed_hop_stand_addition_gets_less() {
    let mut delayed = post_boil_parameters(0.0);
    delayed.post_boil_addition = true;
    delayed.post_boil_delay_minutes = 10.0;
    delayed.cool_time_minutes = Some(20.0);
    assert!(close(ibus(&delayed, Formula::MIbu), 17.644, 0.05), "{}", ibus(&delayed, Formula::MIbu));
}

/// Held at 80 °C the wort still isomerises, just more slowly than at the boil.
#[test]
fn a_profile_that_holds_the_wort_hot_keeps_it_working() {
    let mut held = post_boil_parameters(0.0);
    held.post_boil_addition = true;
    held.cool_time_minutes = Some(30.0);
    held.post_boil_temperature_profile = vec![TemperatureSegment {
        duration_minutes: 60.0,
        start_temp_c: 80.0,
        end_temp_c: 80.0,
        natural_cooling: false,
        starts_at_previous_temp: false,
    }];

    let mut cooling = held.clone();
    cooling.post_boil_temperature_profile.clear();

    let held_ibus = ibus(&held, Formula::MIbu);
    assert!(held_ibus > 0.0, "80 C is still hot enough to isomerise");
    assert!(held_ibus < ibus(&cooling, Formula::MIbu), "but less than wort left to cool from boiling");
}

#[test]
fn the_cooling_model_behaves_physically() {
    // A wider opening loses heat faster.
    assert!(natural_cooling_rate(20.0, 38.0, 38.0) > natural_cooling_rate(20.0, 38.0, 25.0));
    // More wort holds its heat longer.
    assert!(natural_cooling_rate(40.0, 38.0, 38.0) < natural_cooling_rate(20.0, 38.0, 38.0));
    // Isomerisation falls away sharply as the wort cools.
    assert!(relative_utilization(100.0) > relative_utilization(80.0));
    assert!(relative_utilization(80.0) > relative_utilization(60.0));
    assert!(close(relative_utilization(100.0), 1.0, 0.05), "boiling is the reference point");

    // With no profile the wort simply cools along Hosom's curve.  That curve is fitted with a
    // flameout temperature of 100.1 C rather than exactly 100, and the C++ does not clamp this
    // path to boiling -- only the paths that walk a profile segment do.  So the reading at
    // flameout is a tenth of a degree over, which is the model being what it is rather than an
    // error.
    let rate = natural_cooling_rate(19.87, 38.0, 38.0);
    let at_flameout = post_boil_wort_temperature_c(0.0, &[], rate, 100.0);
    let later = post_boil_wort_temperature_c(30.0, &[], rate, 100.0);
    assert!(close(at_flameout, 100.1, 0.01), "{at_flameout}");
    assert!(later < at_flameout, "and it only goes down from there");
    // A profile segment, by contrast, is capped at boiling.
    let held = TemperatureSegment {
        duration_minutes: 10.0,
        start_temp_c: 120.0,
        end_temp_c: 120.0,
        natural_cooling: false,
        starts_at_previous_temp: false,
    };
    assert!(close(post_boil_wort_temperature_c(5.0, &[held], rate, 100.0), 100.0, 1e-9));
}
