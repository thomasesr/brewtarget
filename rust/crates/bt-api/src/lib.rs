// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/lib.rs is part of Brewtarget, and is copyright the following authors 2026:
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

//! HTTP JSON API
//!
//! See `PLAN.md` for the phase this crate belongs to.

#![forbid(unsafe_code)]

pub mod error;
pub mod generated;
pub mod inventory;
pub mod openapi;
pub mod recipes;
pub mod router;
pub mod state;
pub mod transfer;

pub use error::{ApiError, ApiResult};
pub use router::{api, router};
pub use state::{AppState, Preferences};
