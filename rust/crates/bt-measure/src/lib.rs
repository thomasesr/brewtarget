// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-measure/src/lib.rs is part of Brewtarget, and is copyright the following authors 2026:
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

//! Units, physical quantities and conversions
//!
//! Ports from `src/measurement/` in the legacy Qt/C++ tree.
//!
//! See `PLAN.md` for the phase this crate belongs to.

#![forbid(unsafe_code)]

pub mod data;
pub mod formula;
pub mod numeric;
pub mod unit;

pub use data::{UnitData, UnitDef, UnitSystemDef, unit_data};
pub use numeric::{Polynomial, interpolate};
pub use unit::{Amount, Unit, UnitSystem, split_amount_string};
