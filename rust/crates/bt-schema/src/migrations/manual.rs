// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/migrations/manual.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The migrations that could not be lifted as data.
//!
//! `tools/extract-migrations.py` reports these by name rather than skipping them, so this module
//! and `schema/migrations.json` cannot drift apart unnoticed: [`check_coverage`] asserts that the
//! set of migrations without statements is exactly the set implemented here.

use sqlx::AnyConnection;

use super::run::MigrationError;
use crate::dialect::Dialect;
use crate::migrations::model::migrations;

/// The schema versions whose migration is hand-written below.
pub const HAND_PORTED: [u32; 2] = [1, 19];

/// Apply the hand-ported migration that advances `from` by one version.
pub async fn apply(from: u32, connection: &mut AnyConnection, dialect: Dialect) -> Result<(), MigrationError> {
    match from {
        1 => migrate_to_202(connection, dialect).await,
        19 => super::folders::migrate_to_20(connection, dialect).await,
        _ => Err(MigrationError::MissingStep(from)),
    }
}

/// Ported from `migrate_to_202` in `src/database/DatabaseSchemaHelper.cpp`.
///
/// Written there with a `QTextStream` and executed statement by statement rather than through the
/// usual query vector, which is why it is here rather than in `schema/migrations.json`.
async fn migrate_to_202(connection: &mut AnyConnection, dialect: Dialect) -> Result<(), MigrationError> {
    let double = super::render::render("{{type:double}}", dialect)?;
    let integer = super::render::render("{{type:int}}", dialect)?;
    let primary_key = super::render::render("{{primary_key}}", dialect)?;

    for sql in [
        format!("ALTER TABLE brewnote ADD COLUMN projected_ferm_points {double};"),
        // The C++ writes `ALTER TABLE brewnote SET projected_ferm_points = -1.0`, which is not
        // valid SQL in any of our three dialects, so that migration always fails there and no
        // version 1 database can in fact be upgraded.  Nothing is gained by reproducing a
        // statement that cannot run, so the evident intent is used instead.
        "UPDATE brewnote SET projected_ferm_points = -1.0;".to_owned(),
        format!(
            "CREATE TABLE settings (\nid {primary_key},\nrepopulatechildrenonnextstart {integer},\nversion {integer});"
        ),
    ] {
        sqlx::query(&sql).execute(&mut *connection).await.map_err(|source| MigrationError::Statement {
            from: 1,
            to: 2,
            function: "migrate_to_202".to_owned(),
            sql: sql.clone(),
            source,
        })?;
    }
    Ok(())
}

/// Assert that every migration lacking statements has a hand-written implementation.
pub fn check_coverage() -> Result<(), String> {
    let mut missing: Vec<u32> =
        migrations().migrations.iter().filter(|m| m.statements.is_none()).map(|m| m.from_version).collect();
    missing.sort_unstable();
    if missing == HAND_PORTED {
        Ok(())
    } else {
        Err(format!("schema/migrations.json needs hand ports for {missing:?}, but this module covers {HAND_PORTED:?}"))
    }
}
