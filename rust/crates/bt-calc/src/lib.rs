// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/lib.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// Brewtarget is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied
// warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more
// details.
//
// You should have received a copy of the GNU General Public License along with this program.  If not, see
// <http://www.gnu.org/licenses/>.

//! Brewing calculations: gravity, ABV, IBU, colour, mash
//!
//! Ports from `src/calculations/` in the legacy Qt/C++ tree.
//!
//! See `PLAN.md` for the phase this crate belongs to.

#![forbid(unsafe_code)]

pub mod algorithms;
pub mod color;
pub mod data;
pub mod ibu;
pub mod instructions;
pub mod mash;
pub mod mash_adjust;
pub mod recipe;
pub mod recipe_ibu;
pub mod smph;
pub mod water;

pub use color::{ColorFormula, mcu_to_srm, srm_to_display_color};
pub use data::{CalculationData, calculation_data, constant};
pub use ibu::{Breakdown, Formula, HopForm, ibu_breakdown, ibus};
