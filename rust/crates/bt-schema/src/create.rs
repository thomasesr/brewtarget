// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/create.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Build a database at the current schema version.
//!
//! Ported from `DatabaseSchemaHelper::create` in `src/database/DatabaseSchemaHelper.cpp`.  Note
//! that this creates the tables from the schema definitions rather than by running the migrations,
//! exactly as the C++ does: the two routes are expected to agree, and the Phase 2 tests check that
//! they do.

use sqlx::AnyConnection;

use crate::dialect::{Dialect, Namespace};
use crate::migrations::model::migrations;
use crate::migrations::render::bind_placeholders;
use crate::migrations::run::MigrationError;

/// Create every table at schema version 23 and record that version in `settings`.
///
/// The `default_content_version` starts at 0, meaning no default content has been loaded yet.
pub async fn create_database(
    connection: &mut AnyConnection,
    dialect: Dialect,
    namespace: &Namespace,
) -> Result<(), MigrationError> {
    for statement in crate::ddl::create_all(dialect, namespace)? {
        sqlx::query(&statement).execute(&mut *connection).await.map_err(|source| MigrationError::Statement {
            from: 0,
            to: migrations().latest_version,
            function: "DatabaseSchemaHelper::create".to_owned(),
            sql: statement.clone(),
            source,
        })?;
    }

    let sql = bind_placeholders("INSERT INTO settings (version, default_content_version) VALUES (?, ?)", dialect);
    // Both columns are INTEGER, which is 32 bits on PostgreSQL and MariaDB, so the bound values
    // have to be too -- a 64-bit parameter is rejected outright rather than narrowed.
    sqlx::query(&sql)
        .bind(i32::try_from(migrations().latest_version).unwrap_or(i32::MAX))
        .bind(0_i32)
        .execute(&mut *connection)
        .await?;
    Ok(())
}
