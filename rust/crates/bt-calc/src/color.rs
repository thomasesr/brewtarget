// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/color.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Beer colour.
//!
//! Ported from `src/calculations/ColorMethods.cpp`.  Three published approximations turn malt
//! colour units into SRM; which one to use is a user preference, so it is a parameter here rather
//! than the global the C++ keeps it in.

/// Which approximation to use for malt colour units to SRM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorFormula {
    Mosher,
    Daniel,
    /// The C++ default.
    #[default]
    Morey,
}

impl ColorFormula {
    /// The string this is stored as, from `ColorMethods::formulaStringMapping`.
    pub fn as_str(&self) -> &'static str {
        match self {
            ColorFormula::Mosher => "mosher",
            ColorFormula::Daniel => "daniel",
            ColorFormula::Morey => "morey",
        }
    }
}

/// Malt colour units to SRM.
pub fn mcu_to_srm(mcu: f64, formula: ColorFormula) -> f64 {
    match formula {
        ColorFormula::Morey => 1.4922 * mcu.powf(0.6859),
        ColorFormula::Daniel => 0.2 * mcu + 8.4,
        ColorFormula::Mosher => 0.3 * mcu + 4.7,
    }
}

/// The colour to draw a beer of the given SRM, as 8-bit red, green and blue.
///
/// Ported from `ColorMethods::srmToDisplayColor`, which returns a `QColor`.  The odd-looking
/// `+ 0.5` is how the C++ rounds before truncating to an integer, and the red channel is capped
/// at 253 before the general clamp -- both kept as they are.
pub fn srm_to_display_color(srm: f64) -> [u8; 3] {
    let mut r = 0.5 + (272.098 - 5.80255 * srm);
    if r > 253.0 {
        r = 253.0;
    }
    let g = if srm > 35.0 { 0.0 } else { 0.5 + (2.41975e2 - 1.3314e1 * srm + 1.881895e-1 * srm * srm) };
    let b = 0.5 + (179.3 - 28.7 * srm);
    [clamp_channel(r), clamp_channel(g), clamp_channel(b)]
}

fn clamp_channel(value: f64) -> u8 {
    // The C++ truncates to int first, then clamps, so a large value does not wrap.
    let truncated = value as i64;
    truncated.clamp(0, 255) as u8
}
