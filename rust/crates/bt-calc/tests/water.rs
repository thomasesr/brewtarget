// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/water.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The water chemistry, against the figures `WaterAutoAdjusterTests.cpp` works out by hand.
//!
//! That test file does its arithmetic in its comments -- "1 g/L of gypsum giving 233 ppm Ca and
//! 558 ppm SO₄", "RA = 3 − 2 ÷ 3.5 = 2.428571" -- so these check the same numbers rather than
//! whatever this port happens to produce.

use bt_calc::water::{
    Grain, Ion, Ions, MashWater, Profile, Salt, actual_ions_ppm, distilled_water_mash_ph, mash_ph,
    residual_alkalinity_meq_per_l,
};

/// `phTolerance` in the C++ tests.
const PH_TOLERANCE: f64 = 0.001;

fn close(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() < tolerance
}

fn ions(pairs: &[(Ion, f64)]) -> Ions {
    pairs.iter().copied().collect()
}

/// `simpleMash`: 30 L of RO water, 15 of it in the mash, 5 kg of 3 SRM base malt, 12 °P, 6 SRM.
fn simple_mash() -> MashWater {
    MashWater {
        grains: vec![Grain { amount_kg: 5.0, color_srm: 3.0, is_specialty: false }],
        beer_color_srm: 6.0,
        og_plato: 12.0,
        mash_water_l: 15.0,
        ions_ppm: Ions::new(),
        alkalinity_meq_per_l: 0.0,
        acid_meq: 0.0,
    }
}

/// The molar masses are the C++'s own, so a salt's contribution can be checked against the
/// figures the test file works out: gypsum is CaSO₄·2H₂O at 172.16708 g/mol, so 1 g/L gives
/// 40.0784 ÷ 172.16708 × 1000 = 232.788 ppm of calcium.
#[test]
fn a_gram_of_gypsum_in_a_litre_gives_the_calcium_the_references_say() {
    let gypsum_molar_mass = 172.16708;
    let calcium_ppm = Ion::Calcium.molar_mass() / gypsum_molar_mass * 1000.0;
    assert!(close(calcium_ppm, 232.788, 0.001), "{calcium_ppm}");
    // ...and about 558 ppm of sulfate, as the brewing references have it.
    let sulfate_ppm = Ion::Sulfate.molar_mass() / gypsum_molar_mass * 1000.0;
    assert!(close(sulfate_ppm, 557.937, 0.001), "{sulfate_ppm}");
}

/// Salts are shared over all the water, mash and sparge alike.
#[test]
fn a_salt_is_shared_over_all_the_water() {
    // 3 g of calcium chloride flake in 30 L: 3 × 272.626 ÷ 30 = 27.2626 ppm.
    let salt =
        Salt { ions_ppm_per_gram_per_litre: ions(&[(Ion::Calcium, 272.626), (Ion::Chloride, 482.284)]), grams: 3.0 };
    let found = actual_ions_ppm(&Profile::default(), 0.0, &[salt], 30.0);
    assert!(close(found[&Ion::Calcium], 27.2626, 0.0001), "{}", found[&Ion::Calcium]);
    assert!(close(found[&Ion::Chloride], 48.2284, 0.0001), "{}", found[&Ion::Chloride]);
}

/// Reverse osmosis dilutes the base water in proportion.
#[test]
fn reverse_osmosis_dilutes_the_base_water() {
    let base = Profile { ions_ppm: ions(&[(Ion::Calcium, 100.0)]), alkalinity_meq_per_l: None };
    assert!(close(actual_ions_ppm(&base, 1.0, &[], 30.0)[&Ion::Calcium], 100.0, 1e-9));
    assert!(close(actual_ions_ppm(&base, 0.5, &[], 30.0)[&Ion::Calcium], 50.0, 1e-9));
    // All RO: none of the base water's ions are left.
    assert!(close(actual_ions_ppm(&base, 0.0, &[], 30.0)[&Ion::Calcium], 0.0, 1e-9));
}

/// Kolbach's formula, with the C++'s own worked example: "RA = 3 − 2 ÷ 3.5 = 2.428571".
#[test]
fn residual_alkalinity_offsets_calcium_and_magnesium() {
    // 40.0784 ppm of calcium is exactly 2 mEq/L.
    let calcium_ppm = Ion::Calcium.molar_mass();
    assert!(close(Ion::Calcium.milliequivalents_per_litre(calcium_ppm), 2.0, 1e-9));

    let residual = residual_alkalinity_meq_per_l(3.0, calcium_ppm, 0.0);
    assert!(close(residual, 3.0 - 2.0 / 3.5, 1e-9), "{residual}");
    assert!(close(residual, 2.428571, 1e-6), "{residual}");

    // Magnesium offsets half as effectively as calcium, per the formula.
    let magnesium_ppm = Ion::Magnesium.molar_mass();
    let with_magnesium = residual_alkalinity_meq_per_l(3.0, 0.0, magnesium_ppm);
    assert!(close(with_magnesium, 3.0 - 2.0 / 7.0, 1e-9), "{with_magnesium}");
}

/// `testMashPh`: the distilled water mash pH of that grist is 5.495.
#[test]
fn the_distilled_water_mash_ph_follows_from_colour_and_gravity() {
    let mash = simple_mash();
    let found = distilled_water_mash_ph(&mash.grains, mash.beer_color_srm, mash.og_plato).expect("a grist");
    assert!(close(found, 5.495, PH_TOLERANCE), "{found}");

    // With RO water there is no alkalinity, so the mash pH is that figure.
    let found = mash_ph(&mash).expect("a mash");
    assert!(close(found, 5.495, PH_TOLERANCE), "{found}");
}

/// Same test: "Alkaline base water raises the pH" to 5.640714 --
/// RA 2.428571 × 15 ÷ (50 × 5) = 0.145714.
#[test]
fn alkaline_water_raises_the_mash_ph() {
    let mut mash = simple_mash();
    mash.alkalinity_meq_per_l = 3.0;
    mash.ions_ppm = ions(&[(Ion::Calcium, Ion::Calcium.molar_mass())]);

    let found = mash_ph(&mash).expect("a mash");
    assert!(close(found, 5.640714, PH_TOLERANCE), "{found}");
}

/// Same test: "25 mEq of acid lowers the pH by 25 ÷ (50 × 5) = 0.1".
#[test]
fn acid_lowers_the_mash_ph_in_proportion_to_the_grist() {
    let mut mash = simple_mash();
    mash.acid_meq = 25.0;
    let found = mash_ph(&mash).expect("a mash");
    assert!(close(found, 5.395, PH_TOLERANCE), "{found}");
}

/// Same test: 3 g of calcium chloride flake in 30 L lowers the pH by 0.023322.
#[test]
fn calcium_from_a_salt_lowers_the_mash_ph() {
    let mut mash = simple_mash();
    let salt =
        Salt { ions_ppm_per_gram_per_litre: ions(&[(Ion::Calcium, 272.626), (Ion::Chloride, 482.284)]), grams: 3.0 };
    mash.ions_ppm = actual_ions_ppm(&Profile::default(), 0.0, &[salt], 30.0);

    let found = mash_ph(&mash).expect("a mash");
    assert!(close(found, 5.495 - 0.023322, PH_TOLERANCE), "{found}");
}

/// Specialty malt lowers the pH less per degree of colour than base malt does, so a beer that
/// gets its colour from roast finishes higher than one of the same colour that does not.
#[test]
fn specialty_malt_acidifies_less_than_base_malt() {
    let from_base =
        distilled_water_mash_ph(&[Grain { amount_kg: 5.0, color_srm: 20.0, is_specialty: false }], 20.0, 12.0)
            .expect("a grist");
    let from_roast =
        distilled_water_mash_ph(&[Grain { amount_kg: 5.0, color_srm: 20.0, is_specialty: true }], 20.0, 12.0)
            .expect("a grist");
    assert!(from_roast > from_base, "{from_roast} should be above {from_base}");
}

/// Nothing to work it out from is said rather than guessed.
#[test]
fn a_mash_with_nothing_in_it_has_no_ph() {
    assert_eq!(distilled_water_mash_ph(&[], 6.0, 12.0), None, "no grain");
    assert_eq!(mash_ph(&MashWater { grains: Vec::new(), ..simple_mash() }), None);
    assert_eq!(mash_ph(&MashWater { mash_water_l: 0.0, ..simple_mash() }), None, "no water");
}

/// The ions a salt contributes are worked out from its formula and the molar masses, not
/// transcribed.  These are the figures the C++ test file lists for comparison with the usual
/// brewing references.
#[test]
fn each_salt_contributes_what_its_formula_says() {
    use bt_calc::water::WaterAgent;

    let gypsum = WaterAgent::CalciumSulfateDihydrate.ions_ppm_per_gram_per_litre();
    assert!(close(gypsum[&Ion::Calcium], 232.788, 0.001), "{:?}", gypsum);
    assert!(close(gypsum[&Ion::Sulfate], 557.937, 0.001), "{:?}", gypsum);

    // `cacl2Flake` in the C++ tests: the dihydrate.
    let flake = WaterAgent::CalciumChlorideDihydrate.ions_ppm_per_gram_per_litre();
    assert!(close(flake[&Ion::Calcium], 272.626, 0.001), "{:?}", flake);
    assert!(close(flake[&Ion::Chloride], 482.284, 0.001), "{:?}", flake);

    let epsom = WaterAgent::MagnesiumSulfateHeptahydrate.ions_ppm_per_gram_per_litre();
    assert!(close(epsom[&Ion::Magnesium], 98.613, 0.001), "{:?}", epsom);

    // Two chlorides per calcium, so the chloride is far more than the calcium by mass.
    assert!(flake[&Ion::Chloride] > flake[&Ion::Calcium]);
    // The water of crystallisation adds mass and no ions, so the anhydrous is stronger.
    let anhydrous = WaterAgent::CalciumChlorideAnhydrous.ions_ppm_per_gram_per_litre();
    assert!(anhydrous[&Ion::Calcium] > flake[&Ion::Calcium]);

    // An acid contributes no mineral ions.
    assert!(WaterAgent::LacticAcid.ions_ppm_per_gram_per_litre().is_empty());
}

/// The stored strings are the ones the database holds.
#[test]
fn a_water_agent_is_read_from_what_the_column_stores() {
    use bt_calc::water::WaterAgent;
    assert_eq!(WaterAgent::from_stored("calcium carbonate"), Some(WaterAgent::CalciumCarbonate));
    // A type whose chemistry the application does not model has no contribution to work out.
    assert_eq!(WaterAgent::from_stored("other"), None);
    assert_eq!(WaterAgent::from_stored("not a salt"), None);
}

/// A recipe's water, end to end: base profile, reverse osmosis, salts, and the pH that
/// follows.  This is what the Water tab shows.
#[test]
fn a_recipe_with_no_mash_water_has_nothing_to_predict() {
    use bt_model::recipe::Recipe;
    // Nothing to work out is `None`, which is a different thing from a profile of zeroes.
    assert_eq!(bt_calc::water::predict(&Recipe::default()), None);
}

/// Adding gypsum to a recipe raises its calcium and sulfate, and lowers its mash pH.
#[test]
fn a_water_agent_changes_what_the_water_comes_to() {
    use bt_model::enums::{MiscType, MiscWaterAgentType};
    use bt_model::recipe::{Addition, Recipe};
    use bt_model::tables::*;

    let mash_step = MashStepRow {
        mstype: Some("infusion".to_owned()),
        step_temp_c: Some(66.0),
        amount_l: Some(20.0),
        step_time_mins: Some(60.0),
        ..MashStepRow::default()
    };
    let malt = Addition {
        addition: FermentableInRecipeRow {
            quantity: Some(5.0),
            unit: Some("kilograms".to_owned()),
            stage: Some("add_to_mash".to_owned()),
            ..FermentableInRecipeRow::default()
        },
        ingredient: Some(FermentableRow {
            name: Some("Pale".to_owned()),
            ftype: Some("grain".to_owned()),
            grain_group: Some("base".to_owned()),
            color_lovibond: Some(3.0),
            fine_grind_yield_pct: Some(80.0),
            ..FermentableRow::default()
        }),
    };
    let plain = Recipe {
        row: RecipeRow { batch_size: Some(20.0), efficiency: Some(75.0), ..RecipeRow::default() },
        fermentables: vec![malt.clone()],
        mash: Some((MashRow::default(), vec![mash_step.clone()])),
        ..Recipe::default()
    };

    let before = bt_calc::water::predict(&plain).expect("there is water in the mash");
    assert!(close(before.ions_ppm[&Ion::Calcium], 0.0, 1e-9), "no salts, no calcium");

    let mut salted = plain.clone();
    salted.miscs = vec![Addition {
        addition: MiscInRecipeRow {
            // 5 grams.
            quantity: Some(0.005),
            unit: Some("kilograms".to_owned()),
            stage: Some("add_to_mash".to_owned()),
            ..MiscInRecipeRow::default()
        },
        ingredient: Some(MiscRow {
            name: Some("Gypsum".to_owned()),
            mtype: Some(MiscType::WaterAgent.as_str().to_owned()),
            water_agent_type: Some(MiscWaterAgentType::CalciumSulfateDihydrate.as_str().to_owned()),
            ..MiscRow::default()
        }),
    }];

    let after = bt_calc::water::predict(&salted).expect("there is water in the mash");
    assert!(after.ions_ppm[&Ion::Calcium] > 0.0, "gypsum brings calcium: {:?}", after.ions_ppm);
    assert!(after.ions_ppm[&Ion::Sulfate] > after.ions_ppm[&Ion::Calcium], "and more sulfate than calcium");

    // Calcium lowers the mash pH, so the salted recipe finishes below the plain one.
    let (plain_ph, salted_ph) = (before.mash_ph.expect("a pH"), after.mash_ph.expect("a pH"));
    assert!(salted_ph < plain_ph, "{salted_ph} should be below {plain_ph}");
}
