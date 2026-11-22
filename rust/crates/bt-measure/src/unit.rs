// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-measure/src/unit.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Units and conversions.
//!
//! Ported from `src/measurement/Unit.cpp` and `src/measurement/Amount.cpp`.
//!
//! Every physical quantity has one canonical unit, and every other unit of that quantity converts
//! to and from it -- by a scale factor for most, by formula for six (see [`crate::formula`]).
//! Values are held canonically and converted only at the presentation edge, as the C++ does.

use crate::data::{UnitDef, UnitSystemDef, unit_data};
use crate::formula;

/// A handle on one unit.
#[derive(Debug, Clone, Copy)]
pub struct Unit {
    def: &'static UnitDef,
}

// Units are singletons, so identity is equality -- as `Measurement::Unit::operator==` has it.
impl PartialEq for Unit {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.def, other.def)
    }
}
impl Eq for Unit {}

/// A quantity together with the unit it is expressed in.
///
/// Ported from `Measurement::Amount`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Amount {
    pub quantity: f64,
    pub unit: Unit,
}

impl Unit {
    pub fn by_name(name: &str) -> Option<Self> {
        unit_data().unit(name).map(|def| Self { def })
    }

    pub fn name(&self) -> &'static str {
        &self.def.name
    }

    /// The abbreviation shown to the user; a unit may accept others as input, e.g. `°C` and `C`.
    pub fn abbreviation(&self) -> &'static str {
        self.def.abbreviations.first().map(String::as_str).unwrap_or("")
    }

    pub fn abbreviations(&self) -> &'static [String] {
        &self.def.abbreviations
    }

    pub fn unit_system(&self) -> UnitSystem {
        UnitSystem::by_name(&self.def.unit_system).expect("every unit belongs to a known unit system")
    }

    pub fn physical_quantity(&self) -> &'static str {
        &self.unit_system().def.physical_quantity
    }

    /// The unit this one converts through.  A canonical unit is its own canonical unit.
    pub fn canonical(&self) -> Self {
        match &self.def.canonical {
            Some(name) => Self::by_name(name).expect("canonical unit exists"),
            None => *self,
        }
    }

    pub fn is_canonical(&self) -> bool {
        self.def.canonical.is_none()
    }

    /// Convert a quantity of this unit into the canonical unit of its physical quantity.
    pub fn to_canonical(&self, quantity: f64) -> Amount {
        let converted = if self.def.converts_by_formula {
            formula::to_canonical(&self.def.name, quantity).expect("formula for a by-formula unit")
        } else {
            quantity * self.def.scale
        };
        Amount { quantity: converted, unit: self.canonical() }
    }

    /// Convert a quantity of the canonical unit into this one.
    pub fn from_canonical(&self, quantity: f64) -> f64 {
        if self.def.converts_by_formula {
            formula::from_canonical(&self.def.name, quantity).expect("formula for a by-formula unit")
        } else {
            quantity / self.def.scale
        }
    }

    /// Convert a quantity of this unit into `other`, which must measure the same thing.
    ///
    /// Ported from `Measurement::Unit::convertTo`.
    pub fn convert_to(&self, other: Unit, quantity: f64) -> f64 {
        other.from_canonical(self.to_canonical(quantity).quantity)
    }

    /// Rewrite a typed value before parsing; only specific gravity does anything here.
    pub fn sanitise(&self, value: f64) -> f64 {
        if self.def.sanitises_input { formula::sanitise(&self.def.name, value) } else { value }
    }

    /// Above this quantity the display logic prefers the next unit up.
    pub fn boundary_value(&self) -> Option<f64> {
        self.def.boundary_value
    }

    /// Every unit, in declaration order.
    pub fn all() -> impl Iterator<Item = Unit> {
        unit_data().units.iter().map(|def| Unit { def })
    }

    /// The canonical unit for a physical quantity, e.g. kilograms for `Mass`.
    pub fn canonical_for(physical_quantity: &str) -> Option<Unit> {
        Unit::all().find(|unit| unit.is_canonical() && unit.physical_quantity() == physical_quantity)
    }
}

/// A set of units for one physical quantity, ordered smallest to largest.
///
/// Ported from `src/measurement/UnitSystem.cpp`.
#[derive(Debug, Clone, Copy)]
pub struct UnitSystem {
    def: &'static UnitSystemDef,
}

impl PartialEq for UnitSystem {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.def, other.def)
    }
}
impl Eq for UnitSystem {}

impl UnitSystem {
    pub fn by_name(name: &str) -> Option<Self> {
        unit_data().unit_system(name).map(|def| Self { def })
    }

    /// Every unit system, in declaration order.
    pub fn all() -> impl Iterator<Item = Self> {
        unit_data().unit_systems.iter().map(|def| Self { def })
    }

    pub fn name(&self) -> &'static str {
        &self.def.name
    }

    pub fn physical_quantity(&self) -> &'static str {
        &self.def.physical_quantity
    }

    pub fn system_of_measurement(&self) -> &'static str {
        &self.def.system_of_measurement
    }

    pub fn default_unit(&self) -> Unit {
        Unit::by_name(&self.def.default_unit).expect("every unit system has a known default unit")
    }

    /// This system's units, smallest to largest.
    pub fn units(&self) -> impl Iterator<Item = Unit> + '_ {
        self.def.units.iter().filter_map(|name| Unit::by_name(name))
    }

    /// The unit used for mash thickness, where this system has one.
    pub fn thickness(&self) -> Option<Unit> {
        self.def.thickness.as_deref().and_then(Unit::by_name)
    }

    /// Find the unit an abbreviation refers to.
    ///
    /// Ported from `Measurement::Unit::getUnit`.  A unit in this system wins; failing that, any
    /// unit measuring the same physical quantity will do.  That is what makes "3 qt" in a field
    /// marked Imperial mean imperial quarts, while "3 L" in the same field still means litres.
    pub fn find_unit(&self, abbreviation: &str) -> Option<Unit> {
        let matches =
            |unit: &Unit| unit.abbreviations().iter().any(|candidate| candidate.eq_ignore_ascii_case(abbreviation));
        self.units()
            .find(matches)
            .or_else(|| Unit::all().find(|unit| unit.physical_quantity() == self.physical_quantity() && matches(unit)))
    }

    /// Parse user input into a canonical amount.
    ///
    /// Ported from `Measurement::UnitSystem::qstringToSI`.  Input that names no unit, or names one
    /// that means nothing for this physical quantity, is taken to be in `default_unit`.
    pub fn string_to_si(&self, input: &str, default_unit: Unit) -> Amount {
        let (quantity, abbreviation) = split_amount_string(input);
        let unit = abbreviation.and_then(|name| self.find_unit(&name)).unwrap_or(default_unit);
        unit.to_canonical(unit.sanitise(quantity))
    }
}

/// Split "5.500 gal" into its quantity and its unit abbreviation.
///
/// Ported from `Measurement::Unit::splitAmountString`.  Unparseable input is zero, as it is there.
///
/// The C++ takes the decimal point and digit group separator from the user's locale.  Stage 1 is
/// English-only (PLAN.md §7), so this accepts the C locale's `.` and `,`; locale-aware parsing
/// belongs at the presentation edge when translations land.
pub fn split_amount_string(input: &str) -> (f64, Option<String>) {
    let trimmed = input.trim();
    let split_at = trimmed
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',' || c == '-' || c == '+'))
        .unwrap_or(trimmed.len());
    let (number, remainder) = trimmed.split_at(split_at);

    // Group separators are noise once the number has been isolated; a trailing one is not.
    let quantity = number.trim_end_matches(',').replace(',', "").parse::<f64>().unwrap_or(0.0);
    let abbreviation = remainder.trim();
    (quantity, (!abbreviation.is_empty()).then(|| abbreviation.to_owned()))
}
