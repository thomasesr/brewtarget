// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/tests/tools.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The small tools' arithmetic.

use bt_calc::tools::{PrimingSugar, pitch, priming_sugar_g, residual_carbonation_vols};

fn close(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() < tolerance
}

/// Warm beer holds less CO₂, so it needs more sugar for the same carbonation.
#[test]
fn warm_beer_holds_less_carbon_dioxide() {
    // 1.57 × 0.97^t, so at 0 °C it is the full 1.57 volumes.
    assert!(close(residual_carbonation_vols(0.0), 1.57, 1e-9));
    assert!(residual_carbonation_vols(20.0) < residual_carbonation_vols(10.0));

    let cold = priming_sugar_g(19.0, 10.0, 2.4, PrimingSugar::Sucrose);
    let warm = priming_sugar_g(19.0, 20.0, 2.4, PrimingSugar::Sucrose);
    assert!(warm > cold, "{warm} should be more than {cold}");
}

/// The worked example: 19 L at 20 °C to 2.4 volumes with table sugar.
///
/// Residual is 1.57 × 0.97²⁰ = 0.8503 volumes, so 1.5497 have to be made.  That is 29.44 L of
/// CO₂, or 1.3143 moles; sucrose gives four CO₂ per molecule at 342 g/mol, so 112.4 g.
#[test]
fn priming_sugar_is_the_carbon_dioxide_the_beer_still_needs() {
    let residual = 1.57 * 0.97_f64.powf(20.0);
    let co2_mol = (2.4 - residual) * 19.0 / 22.4;

    let sucrose = priming_sugar_g(19.0, 20.0, 2.4, PrimingSugar::Sucrose);
    assert!(close(sucrose, co2_mol / 4.0 * 342.0, 1e-9), "{sucrose}");
    // Around 110 g for a five gallon batch, which is what the references say.
    assert!((100.0..130.0).contains(&sucrose), "{sucrose}");

    // Glucose makes half as much CO₂ per molecule and weighs about half as much, so the two
    // land close together; the monohydrate needs more because the water is dead weight.
    let glucose = priming_sugar_g(19.0, 20.0, 2.4, PrimingSugar::Glucose);
    let monohydrate = priming_sugar_g(19.0, 20.0, 2.4, PrimingSugar::GlucoseMonohydrate);
    assert!(close(glucose, co2_mol / 2.0 * 180.0, 1e-9), "{glucose}");
    assert!(monohydrate > glucose, "the monohydrate carries water: {monohydrate} vs {glucose}");

    // Dry malt extract is about 60% fermentable, so it takes proportionally more.
    let dme = priming_sugar_g(19.0, 20.0, 2.4, PrimingSugar::DryMaltExtract);
    assert!(close(dme, glucose / 0.60, 1e-9), "{dme}");
}

/// Beer that already holds more CO₂ than is wanted needs no sugar, and cannot take negative.
#[test]
fn beer_that_is_already_carbonated_enough_needs_nothing() {
    // 1.0 volumes wanted, but cold beer holds more than that on its own.
    assert_eq!(priming_sugar_g(19.0, 2.0, 1.0, PrimingSugar::Sucrose), 0.0);
    assert_eq!(priming_sugar_g(0.0, 20.0, 2.4, PrimingSugar::Sucrose), 0.0, "no beer, no sugar");
}

/// `PitchDialog::calculate`: cells is the rate times the volume times the gravity.
#[test]
fn the_pitch_is_the_rate_times_the_volume_times_the_gravity() {
    // 20 L of 12 °P at 0.75 million cells per mL per °P -- a typical ale pitch.
    let found = pitch(20.0, 12.0, 0.75, 100.0);
    // 0.75e6 × 20e3 × 12 = 180e9 cells.
    assert!(close(found.cells_billions, 180.0, 1e-9), "{found:?}");
    // At 100 billion per pack, that is 1.8 packs...
    assert!(close(found.vials, 1.8, 1e-9), "{found:?}");
    // ...or 9 grams of dry yeast at 20 billion per gram.
    assert!(close(found.dry_grams, 9.0, 1e-9), "{found:?}");

    // A lager rate is more than twice an ale one, so it needs more of everything.
    let lager = pitch(20.0, 12.0, 2.0, 100.0);
    assert!(lager.cells_billions > found.cells_billions * 2.0);
}

/// Viability is taken into account: a half-dead pack goes twice as far short.
#[test]
fn a_less_viable_pack_means_more_packs() {
    let fresh = pitch(20.0, 12.0, 0.75, 100.0);
    let old = pitch(20.0, 12.0, 0.75, 50.0);
    assert!(close(old.vials, fresh.vials * 2.0, 1e-9), "{old:?} vs {fresh:?}");
    // A pack with nothing in it is not a division by zero.
    assert_eq!(pitch(20.0, 12.0, 0.75, 0.0).vials, 0.0);
}

/// The strike water tool's own formula.
///
/// It declares its own specific heat for barley -- 0.41, where `PhysicalConstants` says 0.4 --
/// so these are its figures, not the mash adjuster's.
#[test]
fn the_strike_water_tool_uses_its_own_formula() {
    use bt_calc::tools::strike_water_temp_c;

    // 3 L/kg, grain at 20 °C, mashing in at 66: (0.41 / 3) × 46 + 66 = 72.286...
    let found = strike_water_temp_c(20.0, 66.0, 3.0).expect("water to grain");
    assert!(close(found, (0.41 / 3.0) * 46.0 + 66.0, 1e-9), "{found}");
    assert!(close(found, 72.2867, 0.0001), "{found}");

    // A thicker mash needs hotter water, because there is less of it to carry the heat.
    let thick = strike_water_temp_c(20.0, 66.0, 2.0).expect("water to grain");
    assert!(thick > found, "{thick} should be above {found}");

    // No water means no temperature for it to be at.
    assert_eq!(strike_water_temp_c(20.0, 66.0, 0.0), None);
}

/// Adding water to a mash already under way.
#[test]
fn a_mash_infusion_is_the_water_the_temperature_rise_needs() {
    use bt_calc::tools::mash_infusion_l;

    // From 66 to 72 with 5 kg of grain in 16 L, using boiling water:
    // (72 − 66) × (0.41 × 5 + 16) ÷ (100 − 72) = 6 × 18.05 ÷ 28.
    let found = mash_infusion_l(66.0, 72.0, 5.0, 100.0, 16.0).expect("boiling water will do it");
    assert!(close(found, 6.0 * 18.05 / 28.0, 1e-9), "{found}");

    // Water at the target temperature changes nothing, however much of it there is.
    assert_eq!(mash_infusion_l(66.0, 72.0, 5.0, 72.0, 16.0), None);
}
