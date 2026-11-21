// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/lib.rs is part of Brewtarget, and is copyright the following authors 2026:
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

//! Canonical database schema, SQL dialects and migrations
//!
//! Ports from `src/database/DatabaseSchemaHelper.cpp` in the legacy Qt/C++ tree.
//!
//! See `PLAN.md` for the phase this crate belongs to.

#![forbid(unsafe_code)]

pub mod create;
pub mod ddl;
pub mod dialect;
pub mod migrations;
pub mod model;

pub use create::create_database;
pub use ddl::{DdlError, add_foreign_keys, create_all, create_table, settings_table};
pub use dialect::{Dialect, InvalidNamespace, Namespace};
pub use migrations::{MigrationError, migrate, migrate_to_latest, migrations, schema_version};
pub use model::{Column, ForeignKey, PrimaryKey, Schema, Table, schema};
