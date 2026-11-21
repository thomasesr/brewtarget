// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/mash.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Mirrors `src/unitTests/MashFermentabilityTests.cpp`.
//!
//! The strongest check is that the model reproduces the published experiments it came from: nine
//! mash schedules with the total sugars the paper's own model predicts.  That is a real external
//! oracle rather than a restatement of the code.

use bt_calc::mash::*;

fn close(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

/// The mash schedule the paper used: a protein rest, a ramp to the rest temperature, the rest
/// itself, then mash-out.
fn paper_profile(rest_temp_c: f64, rest_time_mins: f64) -> Vec<ProfilePoint> {
    let mut profile = vec![ProfilePoint { time_mins: 0.0, temp_c: 37.0 }];
    let mut add = |duration_mins: f64, temp_c: f64| {
        let time_mins = profile.last().expect("not empty").time_mins + duration_mins;
        profile.push(ProfilePoint { time_mins, temp_c });
    };
    add(10.0, 37.0);
    add((50.0 - 37.0) / 2.0, 50.0);
    add(30.0, 50.0);
    add((rest_temp_c - 50.0) / 2.0, rest_temp_c);
    add(rest_time_mins, rest_temp_c);
    add((76.0 - rest_temp_c) / 0.5, 76.0);
    add(10.0, 76.0);
    profile
}

fn infusion(step_temp_c: f64, step_time_mins: f64) -> Step {
    Step { kind: StepType::Infusion, step_temp_c, end_temp_c: None, step_time_mins, ramp_time_mins: None }
}

/// As the C++ test does: default malt and default thickness, which is what the reference mash
/// itself is simulated with.  `EnzymeLevels::default()` is not the same thing -- it is the
/// paper's own malt at the paper's own thickness.
fn multiplier_for(steps: &[Step]) -> f64 {
    attenuation_multiplier(&build_profile(steps), enzyme_levels(None, None)).expect("a non-empty profile")
}

/// The paper's nine experiments, to the tolerance the C++ test uses.
#[test]
fn reproduces_the_paper() {
    for (rest_temp_c, rest_time_mins, paper_model_sugars_g_per_kg, label) in [
        (60.0, 15.0, 103.7, "E1"),
        (63.0, 15.0, 103.0, "E2"),
        (65.0, 15.0, 100.2, "E3"),
        (70.0, 15.0, 94.3, "E4"),
        (63.0, 0.0, 96.0, "E5"),
        (63.0, 60.0, 114.0, "E6"),
        (63.0, 180.0, 123.2, "E7"),
    ] {
        let result = simulate(&paper_profile(rest_temp_c, rest_time_mins), EnzymeLevels::default());
        let sugars = result.fermentable_sugars_g_per_kg();
        assert!(close(sugars, paper_model_sugars_g_per_kg, 1.5), "{label}: {sugars} vs {paper_model_sugars_g_per_kg}");
    }

    // E8 and E9 are isothermal mashes, with no protein rest or mash-out.
    let e8 = simulate(
        &[ProfilePoint { time_mins: 0.0, temp_c: 63.0 }, ProfilePoint { time_mins: 100.0, temp_c: 63.0 }],
        EnzymeLevels::default(),
    );
    assert!(close(e8.fermentable_sugars_g_per_kg(), 116.6, 1.5), "E8: {}", e8.fermentable_sugars_g_per_kg());
    let e9 = simulate(
        &[ProfilePoint { time_mins: 0.0, temp_c: 70.0 }, ProfilePoint { time_mins: 100.0, temp_c: 70.0 }],
        EnzymeLevels::default(),
    );
    assert!(close(e9.fermentable_sugars_g_per_kg(), 92.5, 1.5), "E9: {}", e9.fermentable_sugars_g_per_kg());
}

/// Dextrins are an intermediate: the longer the rest, the more of them get broken down further.
#[test]
fn dextrins_fall_with_a_longer_rest() {
    let mut previous_dextrins = 1000.0;
    for rest_time_mins in [0.0, 15.0, 60.0, 180.0] {
        let dextrins = simulate(&paper_profile(63.0, rest_time_mins), EnzymeLevels::default()).dextrins_g_per_kg;
        assert!(dextrins < previous_dextrins, "{rest_time_mins} minutes gave {dextrins}");
        previous_dextrins = dextrins;
    }
}

/// The reference mash is 66 °C for an hour, so it must come out at a multiplier of one.
#[test]
fn the_reference_mash_is_its_own_yardstick() {
    assert!(close(multiplier_for(&[infusion(66.0, 60.0)]), 1.0, 1e-9));
}

#[test]
fn cooler_and_longer_mashes_are_more_fermentable() {
    let reference = multiplier_for(&[infusion(66.0, 60.0)]);
    // Beta amylase makes maltose and prefers the cooler end, so a low mash ferments further.
    assert!(multiplier_for(&[infusion(62.0, 60.0)]) > reference);
    assert!(multiplier_for(&[infusion(70.0, 60.0)]) < reference);
    // And more time at the same temperature converts more.
    assert!(multiplier_for(&[infusion(66.0, 120.0)]) > reference);
    assert!(multiplier_for(&[infusion(66.0, 30.0)]) < reference);
}

#[test]
fn the_multiplier_is_clamped_to_the_range_the_model_is_trusted_over() {
    // An absurd mash cannot push attenuation anywhere; the model's shape is trusted, not its
    // absolute calibration.
    for steps in [vec![infusion(40.0, 600.0)], vec![infusion(80.0, 600.0)], vec![infusion(55.0, 1.0)]] {
        let multiplier = multiplier_for(&steps);
        assert!((0.81..=1.20).contains(&multiplier), "{multiplier}");
    }
}

#[test]
fn a_sparge_ends_the_mash_and_an_empty_profile_has_no_multiplier() {
    let with_sparge = build_profile(&[
        infusion(66.0, 60.0),
        Step {
            kind: StepType::Sparge,
            step_temp_c: 76.0,
            end_temp_c: None,
            step_time_mins: 20.0,
            ramp_time_mins: None,
        },
    ]);
    let without = build_profile(&[infusion(66.0, 60.0)]);
    assert_eq!(with_sparge, without, "a sparge contributes nothing to the mash");

    assert_eq!(attenuation_multiplier(&[], EnzymeLevels::default()), None);
}

#[test]
fn attenuation_is_capped_but_a_better_yeast_is_not_held_back() {
    // An ordinary yeast cannot be pushed past the cap by a very fermentable mash.
    assert!(adjusted_attenuation_pct(80.0, 1.20) <= 95.0);
    // But a yeast that already attenuates further than the cap keeps its own figure.
    assert_eq!(adjusted_attenuation_pct(98.0, 0.9), 98.0 * 0.9);
    assert_eq!(adjusted_attenuation_pct(75.0, 1.0), 75.0);
}
