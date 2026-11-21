// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/migrations/run.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Apply the migrations to a database.
//!
//! Ported from `migrate`, `migrateNext`, `executeSqlQueries` and `schemaVersion` in
//! `src/database/DatabaseSchemaHelper.cpp`.

use sqlx::{AnyConnection, Connection, Row};

use super::model::{Bind, Condition, Reference, Statement, migrations};
use super::render::{bind_placeholders, render, rewrite_placeholders};
use crate::dialect::Dialect;

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("cannot migrate backwards, from v{from} to v{to}")]
    Backwards { from: u32, to: u32 },
    #[error("v{0} is beyond the latest known schema version v{1}")]
    BeyondLatest(u32, u32),
    #[error("no migration from schema v{0}")]
    MissingStep(u32),
    #[error("migration v{from}->v{to} ({function}) failed on:\n{sql}\n{source}")]
    Statement { from: u32, to: u32, function: String, sql: String, source: sqlx::Error },
    #[error(transparent)]
    Render(#[from] super::render::RenderError),
    #[error(transparent)]
    Ddl(#[from] crate::ddl::DdlError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("could not determine the schema version of this database")]
    UnknownVersion,
}

/// Read `settings.version`, mapping the three string versions the early releases used.
///
/// A database with no `settings` table at all predates version 2.0.2, and so is schema version 1.
pub async fn schema_version(connection: &mut AnyConnection) -> Result<u32, MigrationError> {
    let row = match sqlx::query("SELECT version FROM settings WHERE id = 1").fetch_optional(&mut *connection).await {
        Ok(Some(row)) => row,
        Ok(None) | Err(_) => return Ok(1), // no settings table: version 2.0.0
    };
    // The column held a string in early releases and an integer later, so read it either way.
    if let Ok(version) = row.try_get::<i64, _>("version") {
        return Ok(version as u32);
    }
    let text: String = row.try_get("version").map_err(|_| MigrationError::UnknownVersion)?;
    match text.as_str() {
        "2.0.0" => Ok(1),
        "2.0.2" => Ok(2),
        "2.1.0" => Ok(3),
        // Checked after the string versions above, exactly as the C++ does: an integer parse of
        // "2.0.2" would otherwise quietly yield a wrong answer.
        other => other.parse().map_err(|_| MigrationError::UnknownVersion),
    }
}

/// Bind one value, resolving the two references the extractor leaves for migration time.
fn apply_bind<'q>(
    query: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    bind: &Bind,
    latest_version: u32,
) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
    match bind {
        Bind::Bool(value) => query.bind(*value),
        Bind::Int(value) => query.bind(*value),
        Bind::Float(value) => query.bind(*value),
        Bind::Text(value) => query.bind(value.clone()),
        Bind::Reference { reference: Reference::LatestVersion } => query.bind(i64::from(latest_version)),
        // Inlined as CURRENT_DATE before we get here, so this cannot be reached.
        Bind::Reference { reference: Reference::CurrentDate } => query,
    }
}

/// Run one migration's statements, mirroring `executeSqlQueries`.
async fn run_statements(
    connection: &mut AnyConnection,
    statements: &[Statement],
    dialect: Dialect,
    context: (u32, u32, &str),
) -> Result<(), MigrationError> {
    let (from, to, function) = context;
    let latest = migrations().latest_version;

    // A statement may be conditional on the previous one having returned rows.  When it is
    // skipped, the flag is deliberately left untouched, so that several statements in a row can
    // depend on one trial query -- which is what the C++ does.
    let mut prior_had_results = false;
    for statement in statements {
        if statement.only_if_prior_had_results && !prior_had_results {
            continue;
        }
        if let Some(Condition::ColumnExists(table, column)) = &statement.condition
            && !column_exists(&mut *connection, dialect, table, column).await?
        {
            continue;
        }
        // CURRENT_DATE is written into the statement rather than bound; see rewrite_placeholders.
        let inline: Vec<usize> = statement
            .binds
            .iter()
            .enumerate()
            .filter(|(_, bind)| matches!(bind, Bind::Reference { reference: Reference::CurrentDate }))
            .map(|(index, _)| index)
            .collect();
        let (sql, to_bind) = rewrite_placeholders(&render(&statement.sql, dialect)?, dialect, &inline);
        let mut query = sqlx::query(&sql);
        for index in to_bind {
            query = apply_bind(query, &statement.binds[index], latest);
        }
        let row = query.fetch_optional(&mut *connection).await.map_err(|source| MigrationError::Statement {
            from,
            to,
            function: function.to_owned(),
            sql: sql.clone(),
            source,
        })?;
        prior_had_results = row.is_some();
    }
    Ok(())
}

/// Whether `table` currently has `column`.
///
/// Ported from the `QSqlRecord::contains` checks in `migrate_to_21` and `migrate_to_22`.
pub async fn column_exists(
    connection: &mut AnyConnection,
    dialect: Dialect,
    table: &str,
    column: &str,
) -> Result<bool, MigrationError> {
    let found = match dialect {
        // pragma_table_info takes the table name as a value, so it can still be bound.
        Dialect::Sqlite => {
            sqlx::query("SELECT 1 FROM pragma_table_info(?) WHERE name = ?")
                .bind(table)
                .bind(column)
                .fetch_optional(&mut *connection)
                .await?
        }
        Dialect::Postgresql | Dialect::Mariadb => {
            let sql = bind_placeholders(
                "SELECT 1 FROM information_schema.columns WHERE table_name = ? AND column_name = ?",
                dialect,
            );
            sqlx::query(&sql).bind(table).bind(column).fetch_optional(&mut *connection).await?
        }
    };
    Ok(found.is_some())
}

/// Advance one version, then record the new version exactly as `migrateNext` does.
async fn migrate_next(connection: &mut AnyConnection, from: u32, dialect: Dialect) -> Result<(), MigrationError> {
    let step = migrations().step(from).ok_or(MigrationError::MissingStep(from))?;
    match &step.statements {
        Some(statements) => {
            run_statements(connection, statements, dialect, (from, step.to_version, &step.cpp_function)).await?
        }
        None => super::manual::apply(from, connection, dialect).await?,
    }

    // The C++ skips this below version 4, where `settings` either does not exist yet or still
    // holds a version string that the migration itself has already written.
    if from > 3 {
        let sql = bind_placeholders("UPDATE settings SET version = ? WHERE id = 1", dialect);
        sqlx::query(&sql).bind((from + 1).to_string()).execute(&mut *connection).await?;
    }
    Ok(())
}

/// Migrate a database from `from` to `to`, all or nothing.
///
/// Foreign keys are disabled for the duration, as several migrations rebuild tables that others
/// reference.  SQLite ignores `PRAGMA foreign_keys` inside a transaction, so it is set first.
pub async fn migrate(
    connection: &mut AnyConnection,
    dialect: Dialect,
    from: u32,
    to: u32,
) -> Result<(), MigrationError> {
    let latest = migrations().latest_version;
    if from >= to {
        return Err(MigrationError::Backwards { from, to });
    }
    if to > latest {
        return Err(MigrationError::BeyondLatest(to, latest));
    }

    for sql in disable_foreign_keys(dialect) {
        sqlx::query(sql).execute(&mut *connection).await?;
    }
    let result = async {
        let mut transaction = connection.begin().await?;
        for version in from..to {
            migrate_next(&mut transaction, version, dialect).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
    .await;
    for sql in enable_foreign_keys(dialect) {
        sqlx::query(sql).execute(&mut *connection).await?;
    }
    result
}

/// Migrate a database to the latest schema version, doing nothing if it is already there.
pub async fn migrate_to_latest(connection: &mut AnyConnection, dialect: Dialect) -> Result<u32, MigrationError> {
    let latest = migrations().latest_version;
    let current = schema_version(connection).await?;
    if current < latest {
        migrate(connection, dialect, current, latest).await?;
    }
    Ok(latest)
}

fn disable_foreign_keys(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Sqlite => &["PRAGMA foreign_keys = OFF"],
        Dialect::Postgresql => &["SET session_replication_role = 'replica'"],
        Dialect::Mariadb => &["SET FOREIGN_KEY_CHECKS = 0"],
    }
}

fn enable_foreign_keys(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Sqlite => &["PRAGMA foreign_keys = ON"],
        Dialect::Postgresql => &["SET session_replication_role = 'origin'"],
        Dialect::Mariadb => &["SET FOREIGN_KEY_CHECKS = 1"],
    }
}

/// Whether this database has no Brewtarget tables yet.
///
/// Mirrors the check `Database::load` makes before deciding to create rather than migrate: the
/// `settings` table is the one every version has, under that name, since schema version 2.
pub async fn is_empty(connection: &mut AnyConnection) -> Result<bool, MigrationError> {
    Ok(sqlx::query("SELECT 1 FROM settings WHERE id = 1").fetch_optional(&mut *connection).await.is_err())
}
