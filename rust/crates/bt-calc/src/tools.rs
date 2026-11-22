// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/tools.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/tools/PrimingDialog.cpp and src/tools/PitchDialog.cpp

//! The arithmetic behind the small tools.
//!
//! The hydrometer and refractometer tools are already served by `algorithms`, and strike water
//! by `mash_adjust`.  These are the two that were not: priming and pitching.

/// What sugar is being used to prime with.
///
/// Ported from the radio buttons in `ui/primingDialog.ui`.  Each ferments to a different
/// number of moles of CO₂ per mole of sugar, which is why the amounts differ so much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimingSugar {
    /// Dextrose monohydrate: 198 g/mol, two CO₂ per molecule.
    GlucoseMonohydrate,
    /// Anhydrous dextrose: 180 g/mol.
    Glucose,
    /// Table sugar: 342 g/mol, and four CO₂ per molecule because it is a disaccharide.
    Sucrose,
    /// Dry malt extract, which is about 60% fermentable as glucose.
    DryMaltExtract,
}

impl PrimingSugar {
    /// Grams per mole, and how many moles of CO₂ one mole of it makes.
    fn molar_mass_and_yield(self) -> (f64, f64) {
        match self {
            Self::GlucoseMonohydrate => (198.0, 2.0),
            Self::Glucose => (180.0, 2.0),
            Self::Sucrose => (342.0, 4.0),
            // Treated as glucose at 60% strength.
            Self::DryMaltExtract => (180.0 / 0.60, 2.0),
        }
    }
}

/// How much CO₂ is still dissolved in beer at a given temperature.
///
/// Ported from `PrimingDialog::calculate`.  Warm beer holds less, which is why the same
/// carbonation needs more sugar after a warm conditioning than a cold one.
pub fn residual_carbonation_vols(temp_c: f64) -> f64 {
    1.57 * 0.97_f64.powf(temp_c)
}

/// How much sugar to prime with, in grams.
///
/// Ported from `PrimingDialog::calculate`.  Returns zero rather than a negative amount when
/// the beer already holds more CO₂ than is wanted -- there is no way to prime it down.
pub fn priming_sugar_g(beer_l: f64, temp_c: f64, desired_vols: f64, sugar: PrimingSugar) -> f64 {
    let added_vols = desired_vols - residual_carbonation_vols(temp_c);
    if added_vols <= 0.0 || beer_l <= 0.0 {
        return 0.0;
    }
    // Litres of CO₂ to generate, at 273 K and one atmosphere, then moles of it.
    let co2_l = added_vols * beer_l;
    let co2_mol = co2_l / 22.4;

    let (molar_mass, co2_per_sugar) = sugar.molar_mass_and_yield();
    co2_mol / co2_per_sugar * molar_mass
}

/// What a yeast pitch comes to.
///
/// Ported from `PitchDialog::calculate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    /// Billions of cells wanted.
    pub cells_billions: f64,
    /// Packs or vials of liquid yeast, taking their viability into account.
    pub vials: f64,
    /// Grams of dry yeast, at 20 billion cells per gram.
    pub dry_grams: f64,
}

/// The pitch rate range the dialog's slider covers, in million cells per millilitre per °P.
///
/// `PitchDialog::calculate` maps the slider across this; 0.75 is a typical ale rate and 2.0 a
/// lager one.
pub const PITCH_RATE_RANGE: std::ops::RangeInclusive<f64> = 0.75..=2.0;

/// How much yeast to pitch.
///
/// Ported from `PitchDialog::calculate`: cells wanted is the rate times the volume times the
/// gravity in Plato, with the units lining up as millions per millilitre against litres.
pub fn pitch(volume_l: f64, plato: f64, rate_million_per_ml_per_plato: f64, vial_billions: f64) -> Pitch {
    let cells = (rate_million_per_ml_per_plato * 1e6) * (volume_l * 1e3) * plato;
    Pitch {
        cells_billions: cells / 1e9,
        // A vial or pack is about 100 billion cells before viability is taken off.
        vials: if vial_billions > 0.0 { cells / (vial_billions * 1e9) } else { 0.0 },
        // 20 billion cells per gram of dry yeast.
        dry_grams: cells / 20e9,
    }
}

/// Barley's specific heat, as the strike water tool uses it.
///
/// Ported literally from `StrikeWaterDialog.cpp`, which declares its own `0.41` rather than
/// using `PhysicalConstants::grainSpecificHeat_calGC`, which is `0.4`.  The two disagree in
/// the C++ itself; this reproduces what the tool does so that it gives the same answers, and
/// `mash_adjust` keeps using the constant, as `MashAutoAdjuster` does.
const STRIKE_WATER_GRAIN_SPECIFIC_HEAT: f64 = 0.41;

/// How hot the strike water has to be, as the strike water tool works it out.
///
/// Ported from `initialInfusionSi`.  `water_to_grain` is litres per kilogram.  Returns `None`
/// where there is no water to speak of, rather than dividing by it.
pub fn strike_water_temp_c(grain_temp_c: f64, target_temp_c: f64, water_to_grain_l_kg: f64) -> Option<f64> {
    if water_to_grain_l_kg == 0.0 {
        return None;
    }
    Some((STRIKE_WATER_GRAIN_SPECIFIC_HEAT / water_to_grain_l_kg) * (target_temp_c - grain_temp_c) + target_temp_c)
}

/// How much water to add to bring a mash already under way up to a new temperature.
///
/// Ported from `mashInfusionSi`.  `None` where the infusion water is at the target
/// temperature, since then no amount of it would change anything.
pub fn mash_infusion_l(
    initial_temp_c: f64,
    target_temp_c: f64,
    grain_kg: f64,
    infusion_water_temp_c: f64,
    mash_volume_l: f64,
) -> Option<f64> {
    if infusion_water_temp_c - target_temp_c == 0.0 {
        return None;
    }
    Some(
        ((target_temp_c - initial_temp_c) * (STRIKE_WATER_GRAIN_SPECIFIC_HEAT * grain_kg + mash_volume_l))
            / (infusion_water_temp_c - target_temp_c),
    )
}
