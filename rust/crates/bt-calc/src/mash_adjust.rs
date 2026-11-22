// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/mash_adjust.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/MashAutoAdjuster.cpp

//! Working out a mash schedule: strike water, infusions, sparge.
//!
//! A brewer says what temperatures they want to rest at and for how long; this works out how
//! much water to add and how hot it has to be to get there, given the grain, the tun and what
//! is already in it.  It is the arithmetic behind the Qt application's "auto-adjust" on the
//! mash editor, and behind the strike water tool.
//!
//! Ported from `src/MashAutoAdjuster.cpp`.  The heat model is a mixing calculation repeated
//! down the steps: everything in the tun has a heat capacity and a temperature, water is added
//! at some temperature, and the result is whatever the two settle at.

use crate::data::constant;

/// `PhysicalConstants::waterSpecificHeat_calGC`, in kcal per kg per °C.
fn water_specific_heat() -> f64 {
    constant("waterSpecificHeat_calGC")
}

/// `PhysicalConstants::grainSpecificHeat_calGC`.
fn grain_specific_heat() -> f64 {
    constant("grainSpecificHeat_calGC")
}

/// Close enough to zero to be zero, as `qFuzzyIsNull` has it.
fn is_nothing(value: f64) -> bool {
    value.abs() < 1e-12
}

/// What is in the tun: each part with its heat capacity and its temperature.
///
/// Ported from the `Contents` helper.  Grain, the tun itself and whatever water is already
/// there are all just parts; the arithmetic does not care which is which.
#[derive(Debug, Clone, Default)]
pub struct Contents {
    pub parts: Vec<Part>,
}

/// One thing in the tun.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub heat_capacity_kcal_c: f64,
    pub temp_c: f64,
}

impl Contents {
    pub fn heat_capacity_kcal_c(&self) -> f64 {
        self.parts.iter().map(|part| part.heat_capacity_kcal_c).sum()
    }

    /// The heat needed to bring everything to `temp_c`, negative if it needs cooling.
    pub fn heat_needed_kcal(&self, temp_c: f64) -> f64 {
        self.parts.iter().map(|part| part.heat_capacity_kcal_c * (temp_c - part.temp_c)).sum()
    }

    /// Where the temperature settles after adding `amount_l` of water at `water_temp_c`.
    pub fn final_temp_c(&self, amount_l: f64, water_temp_c: f64) -> f64 {
        let specific_heat = water_specific_heat();
        let mut heat_capacity = amount_l * specific_heat;
        let mut energy = amount_l * specific_heat * water_temp_c;
        for part in &self.parts {
            heat_capacity += part.heat_capacity_kcal_c;
            energy += part.heat_capacity_kcal_c * part.temp_c;
        }
        if heat_capacity > 0.0 { energy / heat_capacity } else { water_temp_c }
    }

    /// How hot the water has to be to reach `temp_c` by adding `amount_l` of it.
    ///
    /// The caller has to have checked that `amount_l` is more than nothing.
    pub fn water_temp_for(&self, temp_c: f64, amount_l: f64) -> f64 {
        temp_c + self.heat_needed_kcal(temp_c) / (amount_l * water_specific_heat())
    }

    /// How much water at `water_temp_c` it takes to reach `temp_c`.
    ///
    /// `None` when it cannot be done at all -- water no hotter than the target cannot heat
    /// the mash, however much of it there is.
    pub fn amount_for(&self, temp_c: f64, water_temp_c: f64) -> Option<f64> {
        let heat_needed_kcal = self.heat_needed_kcal(temp_c);
        let water_temp_difference_c = water_temp_c - temp_c;
        if is_nothing(heat_needed_kcal) {
            return Some(0.0);
        }
        if is_nothing(water_temp_difference_c) {
            return None;
        }
        let amount_l = heat_needed_kcal / (water_specific_heat() * water_temp_difference_c);
        if amount_l < 0.0 { None } else { Some(amount_l) }
    }
}

/// What the mash is made of, before any water goes in.
///
/// Ported from `MashAutoAdjuster::Inputs`, less the parts only the full scheduler needs.
#[derive(Debug, Clone, Copy)]
pub struct Mashing {
    pub grain_kg: f64,
    /// The temperature the grain starts at, which is also taken to be the temperature of the
    /// coldest water available -- it is the temperature of the room.
    pub grain_temp_c: f64,
    /// The heat the tun itself soaks up, which is why a cold tun needs hotter strike water.
    pub tun_heat_capacity_kcal_c: f64,
    pub tun_temp_c: f64,
    pub boiling_point_c: f64,
}

impl Default for Mashing {
    fn default() -> Self {
        Self {
            grain_kg: 0.0,
            grain_temp_c: 20.0,
            tun_heat_capacity_kcal_c: 0.0,
            tun_temp_c: 20.0,
            boiling_point_c: 100.0,
        }
    }
}

impl Mashing {
    /// Everything in the tun before the first infusion: the grain and the tun.
    pub fn before_mashing_in(&self) -> Contents {
        Contents {
            parts: vec![
                Part { heat_capacity_kcal_c: self.grain_kg * grain_specific_heat(), temp_c: self.grain_temp_c },
                Part { heat_capacity_kcal_c: self.tun_heat_capacity_kcal_c, temp_c: self.tun_temp_c },
            ],
        }
    }

    /// Everything in the tun once the mash is under way, which settles to one temperature.
    ///
    /// The grain, the tun and the water already in it are no longer at different temperatures,
    /// so they are one part rather than three.
    pub fn during_mash(&self, water_l: f64, temp_c: f64) -> Contents {
        Contents {
            parts: vec![Part {
                heat_capacity_kcal_c: self.grain_kg * grain_specific_heat()
                    + water_l * water_specific_heat()
                    + self.tun_heat_capacity_kcal_c,
                temp_c,
            }],
        }
    }

    /// How hot the strike water has to be to mash in at `target_temp_c`.
    ///
    /// This is what the strike water tool asks and the first step of every infusion mash
    /// needs.  `None` when no water is being added, since then there is nothing to be hot.
    pub fn strike_water_temp_c(&self, target_temp_c: f64, water_l: f64) -> Option<f64> {
        if water_l <= 0.0 {
            return None;
        }
        Some(self.before_mashing_in().water_temp_for(target_temp_c, water_l))
    }

    /// How much water at `water_temp_c` it takes to mash in at `target_temp_c`.
    pub fn strike_water_amount_l(&self, target_temp_c: f64, water_temp_c: f64) -> Option<f64> {
        self.before_mashing_in().amount_for(target_temp_c, water_temp_c)
    }

    /// Whether a temperature is one water can be at: above freezing, below boiling.
    ///
    /// `temperatureMakesSense` in the C++, which uses it to reject a mash that cannot happen
    /// before trying to work one out.
    pub fn temperature_makes_sense(&self, temp_c: f64) -> bool {
        0.0 < temp_c && temp_c < self.boiling_point_c
    }
}
