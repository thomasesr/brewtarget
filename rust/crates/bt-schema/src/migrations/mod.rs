// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/migrations/mod.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The schema upgrade path, from version 1 to 23.
//!
//! Most of it is data: `tools/extract-migrations.py` lifts 814 statements out of
//! `src/database/DatabaseSchemaHelper.cpp` into `schema/migrations.json`, with the
//! dialect-specific parts left as placeholders.  Two migrations are written as code there rather
//! than as a list of queries, and are hand-ported in [`manual`] and [`folders`].

pub mod folders;
pub mod manual;
pub mod model;
pub mod render;
pub mod run;

pub use model::{Bind, Condition, Migration, Migrations, Reference, Statement, migrations};
pub use render::{RenderError, render};
pub use run::{MigrationError, column_exists, migrate, migrate_to_latest, schema_version};
