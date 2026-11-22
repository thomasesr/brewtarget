// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/water.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/WaterAutoAdjuster.cpp

//! What the water in the mash comes to, and what pH it gives.
//!
//! Ported from `src/WaterAutoAdjuster.cpp`, whose own class comment sets out both models.
//!
//! The salt model is additive: a salt dissolves and contributes each of its ions in a fixed
//! ratio, so what a brewer ends up with is the base water, diluted by however much of it is
//! reverse osmosis, plus whatever the additions bring.
//!
//! The mash pH model is Kai Troester's, from
//! <http://braukaiser.com/wiki/index.php/Beer_color_to_mash_pH_(v2.0)>.

use std::collections::BTreeMap;

use crate::data::constant;

/// The six ions brewers care about, plus carbonate, which contributes to alkalinity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ion {
    Calcium,
    Magnesium,
    Sodium,
    Chloride,
    Sulfate,
    Bicarbonate,
    Carbonate,
}

impl Ion {
    /// The six the Water tab shows, in the order it shows them.
    pub const SHOWN: [Self; 6] =
        [Self::Calcium, Self::Magnesium, Self::Sodium, Self::Chloride, Self::Sulfate, Self::Bicarbonate];

    /// What this ion is called in `PhysicalConstants.h`, where its molar mass is.
    fn constant_name(self) -> &'static str {
        match self {
            Self::Calcium => "Calcium",
            Self::Magnesium => "Magnesium",
            Self::Sodium => "Sodium",
            Self::Chloride => "Chloride",
            Self::Sulfate => "Sulfate",
            Self::Bicarbonate => "Bicarbonate",
            Self::Carbonate => "Carbonate",
        }
    }

    /// Grams per mole.
    pub fn molar_mass(self) -> f64 {
        constant(self.constant_name())
    }

    /// How many charges the ion carries, which is what turns ppm into milliequivalents.
    fn charge(self) -> f64 {
        match self {
            // Divalent.
            Self::Calcium | Self::Magnesium | Self::Sulfate | Self::Carbonate => 2.0,
            Self::Sodium | Self::Chloride | Self::Bicarbonate => 1.0,
        }
    }

    /// Milliequivalents per litre, from parts per million.
    pub fn milliequivalents_per_litre(self, ppm: f64) -> f64 {
        ppm / self.molar_mass() * self.charge()
    }
}

/// How much of each ion is in some water, in parts per million.
pub type Ions = BTreeMap<Ion, f64>;

/// What is in the water before anything is added to it.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub ions_ppm: Ions,
    /// Alkalinity as recorded on the profile, in milliequivalents per litre.  Worked out from
    /// the bicarbonate where the profile does not say.
    pub alkalinity_meq_per_l: Option<f64>,
}

impl Profile {
    pub fn ppm(&self, ion: Ion) -> f64 {
        self.ions_ppm.get(&ion).copied().unwrap_or(0.0)
    }

    /// The alkalinity the profile records, or what its bicarbonate implies.
    pub fn alkalinity(&self) -> f64 {
        self.alkalinity_meq_per_l
            .unwrap_or_else(|| Ion::Bicarbonate.milliequivalents_per_litre(self.ppm(Ion::Bicarbonate)))
    }
}

/// A salt added to the water, and how much of it.
#[derive(Debug, Clone)]
pub struct Salt {
    /// What it contributes, in parts per million per gram per litre.
    pub ions_ppm_per_gram_per_litre: Ions,
    pub grams: f64,
}

/// What the water comes to.
///
/// Ported from the salt model in the class comment: the base water, diluted by whatever
/// proportion of the water is reverse osmosis, plus what each addition brings, shared over all
/// the water -- salts go into the mash and the sparge alike.
pub fn actual_ions_ppm(base: &Profile, base_fraction: f64, salts: &[Salt], total_water_l: f64) -> Ions {
    let base_fraction = base_fraction.clamp(0.0, 1.0);
    let mut found = Ions::new();
    for ion in
        [Ion::Calcium, Ion::Magnesium, Ion::Sodium, Ion::Chloride, Ion::Sulfate, Ion::Bicarbonate, Ion::Carbonate]
    {
        let mut ppm = base_fraction * base.ppm(ion);
        if total_water_l > 0.0 {
            for salt in salts {
                ppm += salt.grams * salt.ions_ppm_per_gram_per_litre.get(&ion).copied().unwrap_or(0.0) / total_water_l;
            }
        }
        found.insert(ion, ppm);
    }
    found
}

/// Residual alkalinity, by Kolbach's formula.
///
/// `RA = alkalinity − Ca²⁺/3.5 − Mg²⁺/7`, all in milliequivalents per litre.  Calcium and
/// magnesium react with the malt's phosphates and release acid, so they offset alkalinity --
/// calcium about twice as effectively as magnesium.
pub fn residual_alkalinity_meq_per_l(alkalinity_meq_per_l: f64, calcium_ppm: f64, magnesium_ppm: f64) -> f64 {
    let calcium = Ion::Calcium.milliequivalents_per_litre(calcium_ppm);
    let magnesium = Ion::Magnesium.milliequivalents_per_litre(magnesium_ppm);
    alkalinity_meq_per_l - calcium / 3.5 - magnesium / 7.0
}

/// `WaterAutoAdjuster::noColorMash_pH`: the pH a mash of colourless malt would land at.
const NO_COLOUR_MASH_PH: f64 = 5.6;
/// `WaterAutoAdjuster::mashBufferCapacity_mEqPerKgPh`: how much acid it takes to move the pH
/// of a kilogram of grist by one.
const MASH_BUFFER_CAPACITY_MEQ_PER_KG_PH: f64 = 50.0;

/// One grain in the mash, as the pH model sees it.
#[derive(Debug, Clone, Copy)]
pub struct Grain {
    pub amount_kg: f64,
    pub color_srm: f64,
    /// Crystal and roasted malts, whose colour lowers the pH less than a base malt's does.
    pub is_specialty: bool,
}

/// The pH a mash of this grist would reach in distilled water.
///
/// Ported from `WaterAutoAdjuster::distilledWaterMashPh`.  Darker malt is more acidic, so the
/// pH falls with colour -- but specialty malt contributes less acidity per degree of colour
/// than base malt, so the two are weighted differently.
///
/// `None` when there is no grain to judge.
pub fn distilled_water_mash_ph(grains: &[Grain], beer_color_srm: f64, og_plato: f64) -> Option<f64> {
    let total_kg: f64 = grains.iter().map(|grain| grain.amount_kg.max(0.0)).sum();
    if total_kg <= 0.0 {
        return None;
    }

    // Degrees Lovibond, as in Troester's spreadsheet.
    let mut weighted_color_lovibond = 0.0;
    let mut specialty_color_lovibond = 0.0;
    for grain in grains {
        let share = grain.amount_kg.max(0.0) / total_kg;
        let color_lovibond = (grain.color_srm + 0.6) / 1.35;
        weighted_color_lovibond += share * color_lovibond;
        if grain.is_specialty {
            specialty_color_lovibond += share * color_lovibond;
        }
    }
    let specialty_ratio =
        if weighted_color_lovibond > 0.0 { specialty_color_lovibond / weighted_color_lovibond } else { 0.0 };

    if og_plato <= 0.0 {
        return Some(NO_COLOUR_MASH_PH);
    }
    const PH_SLOPE_LIGHT: f64 = 0.21;
    const PH_SLOPE_DARK: f64 = 0.06;
    let adjustment =
        (PH_SLOPE_LIGHT * (1.0 - specialty_ratio) + PH_SLOPE_DARK * specialty_ratio) * beer_color_srm / og_plato;
    Some(NO_COLOUR_MASH_PH - adjustment)
}

/// Everything the mash pH depends on.
#[derive(Debug, Clone)]
pub struct MashWater {
    pub grains: Vec<Grain>,
    pub beer_color_srm: f64,
    pub og_plato: f64,
    /// How much water is in the mash itself, as distinct from the sparge.
    pub mash_water_l: f64,
    /// The ions in that water, after any salts.
    pub ions_ppm: Ions,
    /// The alkalinity of that water, in milliequivalents per litre.
    pub alkalinity_meq_per_l: f64,
    /// Acid already in the mash, from acid additions and acidulated malt.
    pub acid_meq: f64,
}

/// What pH the mash will reach.
///
/// Ported from `WaterAutoAdjuster::mashPh`:
///
/// ```text
///     pH = distilled water mash pH + (RA × mash water litres − acid mEq) ÷ (B × grain kg)
/// ```
///
/// `None` where there is nothing to work it out from -- no grain, or no water.
pub fn mash_ph(water: &MashWater) -> Option<f64> {
    let distilled_ph = distilled_water_mash_ph(&water.grains, water.beer_color_srm, water.og_plato)?;
    let grain_kg: f64 = water.grains.iter().map(|grain| grain.amount_kg.max(0.0)).sum();
    if grain_kg <= 0.0 || water.mash_water_l <= 0.0 {
        return None;
    }

    let ppm = |ion: Ion| water.ions_ppm.get(&ion).copied().unwrap_or(0.0);
    let residual = residual_alkalinity_meq_per_l(water.alkalinity_meq_per_l, ppm(Ion::Calcium), ppm(Ion::Magnesium));
    let net_meq = residual * water.mash_water_l - water.acid_meq;
    Some(distilled_ph + net_meq / (MASH_BUFFER_CAPACITY_MEQ_PER_KG_PH * grain_kg))
}
