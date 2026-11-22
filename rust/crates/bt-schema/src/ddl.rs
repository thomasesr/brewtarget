// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/ddl.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Render `schema/v23.json` as DDL.
//!
//! This is a literal reimplementation of `createTableInDb` and `addForeignKeysToTable` in
//! `src/database/ObjectStore.cpp`, including their exact whitespace, so that output for SQLite and
//! PostgreSQL can be compared against what the C++ produces. See PLAN.md Phase 1.
//!
//! Three details of the C++ are load-bearing and reproduced deliberately:
//!
//! - The first field of a table is its primary key, and takes the dialect's primary key
//!   declaration (`INTEGER PRIMARY KEY`, `SERIAL PRIMARY KEY`, …) in place of a type name.
//! - Foreign key columns are omitted from `CREATE TABLE` and added afterwards by
//!   [`add_foreign_keys`], so tables can be created in any order.
//! - No column is `NOT NULL`.
//!
//! Column identifiers are emitted unquoted, as the C++ does. The schema's only column name that
//! is a keyword anywhere is `year`, which is non-reserved in all three dialects.

use crate::dialect::{Dialect, Namespace};
use crate::model::{Table, schema};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DdlError {
    #[error("schema/v23.json has no {kind} entry for dialect {dialect}")]
    MissingDialectEntry { kind: &'static str, dialect: Dialect },
    #[error("schema/v23.json has no native type for base type {0:?}")]
    UnknownBaseType(String),
}

fn lookup<'a>(
    map: &'a std::collections::BTreeMap<String, String>,
    dialect: Dialect,
    kind: &'static str,
) -> Result<&'a str, DdlError> {
    map.get(dialect.key()).map(String::as_str).ok_or(DdlError::MissingDialectEntry { kind, dialect })
}

/// Render the `CREATE TABLE` statement for one table.
pub fn create_table(table: &Table, dialect: Dialect, namespace: &Namespace) -> Result<String, DdlError> {
    let schema = schema();
    let quote = lookup(&schema.identifier_quote, dialect, "identifier_quote")?;
    let primary_key = lookup(&schema.primary_key_declaration, dialect, "primary_key_declaration")?;

    // The C++ writes ", \n" between fields -- note the space before the newline. Reproduced so
    // that generated DDL can be diffed against a dump from the C++ application.
    let mut sql =
        format!("CREATE TABLE {} (\n{} {}", namespace.qualify(&table.name, quote), table.primary_key.name, primary_key);
    for column in &table.columns {
        let native = schema
            .native_types
            .get(&column.base_type)
            .ok_or_else(|| DdlError::UnknownBaseType(column.base_type.clone()))?;
        sql.push_str(", \n");
        sql.push_str(&column.name);
        sql.push(' ');
        sql.push_str(lookup(native, dialect, "native_types")?);
    }
    sql.push_str("\n);");
    Ok(sql)
}

/// Render the `ALTER TABLE` statements that add one table's foreign key columns.
///
/// These run after every table exists, which is why table creation order does not matter.
pub fn add_foreign_keys(table: &Table, dialect: Dialect, namespace: &Namespace) -> Result<Vec<String>, DdlError> {
    let schema = schema();
    let quote = lookup(&schema.identifier_quote, dialect, "identifier_quote")?;
    let template = lookup(&schema.add_foreign_key, dialect, "add_foreign_key")?;

    Ok(table
        .foreign_keys
        .iter()
        .map(|fk| {
            template
                .replace("{table}", &namespace.qualify(&table.name, quote))
                .replace("{column}", &fk.column)
                .replace("{ref_table}", &namespace.qualify(&fk.references_table, quote))
                .replace("{ref_column}", &fk.references_column)
        })
        .collect())
}

/// Render the `settings` table.
///
/// `DatabaseSchemaHelper::create` makes this one by hand rather than from the `ObjectStore`
/// definitions, because only the schema helper and the default content loader ever touch it.  It
/// therefore does not appear in `schema/v23.json`, but a usable database still needs it -- it is
/// where the schema version lives.
pub fn settings_table(dialect: Dialect, namespace: &Namespace) -> Result<String, DdlError> {
    let schema = schema();
    let quote = lookup(&schema.identifier_quote, dialect, "identifier_quote")?;
    let primary_key = lookup(&schema.primary_key_declaration, dialect, "primary_key_declaration")?;
    let integer = lookup(
        schema.native_types.get("int").ok_or_else(|| DdlError::UnknownBaseType("int".into()))?,
        dialect,
        "native_types",
    )?;
    Ok(format!(
        "CREATE TABLE {} (id {primary_key}, version {integer}, default_content_version {integer})",
        namespace.qualify("settings", quote)
    ))
}

/// Render the whole schema: every `CREATE TABLE`, then every foreign key.
pub fn create_all(dialect: Dialect, namespace: &Namespace) -> Result<Vec<String>, DdlError> {
    let tables = &schema().tables;
    let mut statements = Vec::with_capacity(tables.len() * 2);
    for table in tables {
        statements.push(create_table(table, dialect, namespace)?);
    }
    for table in tables {
        statements.extend(add_foreign_keys(table, dialect, namespace)?);
    }
    statements.push(settings_table(dialect, namespace)?);
    Ok(statements)
}

/// The statement that brings a namespace into being, if the dialect has one.
///
/// SQLite has no namespaces at all -- a database is a file -- so there is nothing to create.
/// PostgreSQL has schemas.  MariaDB has no schema-versus-database distinction: `CREATE SCHEMA` is
/// a synonym for `CREATE DATABASE`, so the same isolation is a database there.
///
/// A namespace name cannot be bound as a parameter, so it reaches SQL as text.  That is why
/// [`Namespace::named`] validates it and why this is the only place, along with
/// [`Namespace::qualify`], where the interpolation happens.
pub fn create_namespace(dialect: Dialect, namespace: &Namespace) -> Result<Option<String>, DdlError> {
    let Namespace::Named(name) = namespace else { return Ok(None) };
    let quote = lookup(&schema().identifier_quote, dialect, "identifier_quote")?;
    Ok(match dialect {
        Dialect::Sqlite => None,
        Dialect::Postgresql => Some(format!("CREATE SCHEMA IF NOT EXISTS {quote}{name}{quote}")),
        Dialect::Mariadb => Some(format!("CREATE DATABASE IF NOT EXISTS {quote}{name}{quote}")),
    })
}

/// The statement that removes a namespace and everything in it.
///
/// The counterpart to [`create_namespace`], and subject to the same reasoning about interpolation.
pub fn drop_namespace(dialect: Dialect, namespace: &Namespace) -> Result<Option<String>, DdlError> {
    let Namespace::Named(name) = namespace else { return Ok(None) };
    let quote = lookup(&schema().identifier_quote, dialect, "identifier_quote")?;
    Ok(match dialect {
        Dialect::Sqlite => None,
        Dialect::Postgresql => Some(format!("DROP SCHEMA IF EXISTS {quote}{name}{quote} CASCADE")),
        Dialect::Mariadb => Some(format!("DROP DATABASE IF EXISTS {quote}{name}{quote}")),
    })
}
