// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-measure/src/formula.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The six units that convert by formula rather than by a scale factor.
//!
//! Ported from the lambdas in `src/measurement/Unit.cpp`, and from `SG_20C20C_toPlato`,
//! `PlatoToSG_20C20C`, `SgAt20CToBrix` and `BrixToSgAt20C` in `src/calculations/Algorithms.cpp`.

use crate::data::unit_data;
use crate::numeric::{Polynomial, interpolate};

/// Columns of the USDA sucrose table.
const DEGREES_BRIX: usize = 1;
const APPARENT_SG: usize = 2;

/// Plausible bounds on specific gravity, used to bracket the root search.
const MIN_PLAUSIBLE_SPECIFIC_GRAVITY: f64 = 0.900;
const MAX_PLAUSIBLE_SPECIFIC_GRAVITY: f64 = 1.150;

fn plato_from_sg_polynomial() -> Polynomial {
    Polynomial::new(unit_data().polynomials["platoFromSG_20C20C"].clone())
}

/// Plato from specific gravity, measured at 20 °C relative to the density of water at 20 °C.
pub fn sg_20c20c_to_plato(sg: f64) -> f64 {
    plato_from_sg_polynomial().eval(sg)
}

/// Specific gravity from Plato, by finding where the cubic fit above takes that value.
pub fn plato_to_sg_20c20c(plato: f64) -> f64 {
    plato_from_sg_polynomial()
        .minus_constant(plato)
        .root_find(MIN_PLAUSIBLE_SPECIFIC_GRAVITY, MAX_PLAUSIBLE_SPECIFIC_GRAVITY)
}

/// Brix from specific gravity at 20 °C.
///
/// Brix measures the sugar content of an aqueous solution, so below 1.000 there is nothing
/// meaningful to convert and the C++ returns zero after warning.  Rather than fit a curve, both
/// directions interpolate the 801 observed USDA data points.
pub fn sg_at_20c_to_brix(sg: f64) -> f64 {
    if sg <= 1.0 {
        return 0.0;
    }
    interpolate(&unit_data().sucrose_conversions, sg, APPARENT_SG, DEGREES_BRIX)
}

/// Specific gravity at 20 °C from Brix.
pub fn brix_to_sg_at_20c(brix: f64) -> f64 {
    interpolate(&unit_data().sucrose_conversions, brix, DEGREES_BRIX, APPARENT_SG)
}

/// Convert `quantity` of the named unit into its canonical unit.
///
/// Returns `None` for a unit that is not one of the six, so callers cannot silently fall through
/// to a scale factor that does not apply.
pub fn to_canonical(unit_name: &str, quantity: f64) -> Option<f64> {
    Some(match unit_name {
        "fahrenheit" => (quantity - 32.0) * 5.0 / 9.0,
        "lovibond" => (1.3546 * quantity) - 0.76,
        "gravityPoints" => 1.0 + (quantity / 1000.0),
        "plato" => {
            if quantity == 0.0 {
                0.0
            } else {
                plato_to_sg_20c20c(quantity)
            }
        }
        "brix" => {
            if quantity == 0.0 {
                0.0
            } else {
                brix_to_sg_at_20c(quantity)
            }
        }
        "wk" => (quantity + 16.0) / 3.5,
        _ => return None,
    })
}

/// Convert `quantity` of the canonical unit into the named unit.
pub fn from_canonical(unit_name: &str, quantity: f64) -> Option<f64> {
    Some(match unit_name {
        "fahrenheit" => (quantity * 9.0 / 5.0) + 32.0,
        "lovibond" => (quantity + 0.76) / 1.3546,
        "gravityPoints" => (quantity - 1.0) * 1000.0,
        "plato" => {
            if quantity == 0.0 {
                0.0
            } else {
                sg_20c20c_to_plato(quantity)
            }
        }
        "brix" => {
            if quantity == 0.0 {
                0.0
            } else {
                sg_at_20c_to_brix(quantity)
            }
        }
        "wk" => (3.5 * quantity) - 16.0,
        _ => return None,
    })
}

/// Rewrite what the user typed before parsing it.
///
/// Only specific gravity does this: it turns "1023" into "1.023", and keeps dividing until the
/// value is below ten, so "10235" becomes "1.0235" too.
pub fn sanitise(unit_name: &str, mut value: f64) -> f64 {
    if unit_name == "specificGravity" {
        while value >= 10.0 {
            value /= 10.0;
        }
    }
    value
}
