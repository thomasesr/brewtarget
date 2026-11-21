// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/rows.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Storing a row whose table is only known at run time.
//!
//! The generated repositories are typed, which is what most code should use.  Importing a file is
//! the exception: the reader produces a column-keyed map and the table name as a string, and
//! writing a `match` over thirty-nine tables to get back to a typed call would be a great deal of
//! code saying nothing.
//!
//! Table and column names cannot be bound as parameters, so both are checked against the
//! generated lists before being interpolated.  Nothing here accepts a name the schema does not
//! have.

use bt_model::tables::{columns_of, table_for_cpp_type};
use serde_json::{Map, Value};

use crate::{Connection, Store, StoreError, dispatch, dispatch_connection};

/// Check a table name against the schema.
fn checked_table(table: &str) -> Result<&'static str, StoreError> {
    bt_model::tables::TABLES
        .iter()
        .copied()
        .find(|known| *known == table)
        .ok_or_else(|| StoreError::UnknownTable(table.to_owned()))
}

/// Check a column name against the table it is meant to be in.
fn checked_column(table: &str, column: &str) -> Result<&'static str, StoreError> {
    columns_of(table)
        .and_then(|columns| columns.iter().copied().find(|known| *known == column))
        .ok_or_else(|| StoreError::UnknownColumn { table: table.to_owned(), column: column.to_owned() })
}

/// Bind one JSON value, which is as much type information as an imported row carries.
macro_rules! bind_value {
    ($query:expr, $value:expr) => {
        match $value {
            Value::Null => $query.bind(None::<String>),
            Value::Bool(flag) => $query.bind(*flag),
            Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    $query.bind(integer)
                } else {
                    $query.bind(number.as_f64().unwrap_or_default())
                }
            }
            Value::String(text) => $query.bind(text.clone()),
            // An array or object has no column to go in; the reader does not produce one.
            other => $query.bind(other.to_string()),
        }
    };
}

/// Insert a row described by its table and columns, on a single connection.
///
/// Work that has to be atomic runs on one connection so that a transaction means anything, which
/// is why this exists alongside the pool-based [`insert`].
pub async fn insert_on(
    connection: &mut Connection,
    placeholders: impl Fn(&str) -> String,
    table: &str,
    columns: &Map<String, Value>,
) -> Result<i64, StoreError> {
    let (sql, values) = insert_statement(placeholders, table, columns)?;
    Ok(dispatch_connection!(connection, |handle| {
        let mut query = sqlx::query_scalar(&sql);
        for value in &values {
            query = bind_value!(query, value);
        }
        query.fetch_one(&mut **handle).await?
    }))
}

/// Build the statement and the values it binds.
fn insert_statement<'a>(
    placeholders: impl Fn(&str) -> String,
    table: &str,
    columns: &'a Map<String, Value>,
) -> Result<(String, Vec<&'a Value>), StoreError> {
    let table = checked_table(table)?;
    let primary_key = columns_of(table).and_then(|columns| columns.first()).copied().unwrap_or("id");

    let named: Vec<(&'static str, &Value)> = columns
        .iter()
        .filter(|(name, _)| name.as_str() != primary_key)
        .map(|(name, value)| checked_column(table, name).map(|column| (column, value)))
        .collect::<Result<_, _>>()?;

    if named.is_empty() {
        return Ok((format!("INSERT INTO {table} DEFAULT VALUES RETURNING {primary_key}"), Vec::new()));
    }
    let column_list = named.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
    let marks = named.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = placeholders(&format!("INSERT INTO {table} ({column_list}) VALUES ({marks}) RETURNING {primary_key}"));
    Ok((sql, named.into_iter().map(|(_, value)| value).collect()))
}

/// Insert a row described by its table and columns, returning the key the database generated.
pub async fn insert(store: &Store, table: &str, columns: &Map<String, Value>) -> Result<i64, StoreError> {
    let table = checked_table(table)?;
    let primary_key = columns_of(table).and_then(|columns| columns.first()).copied().unwrap_or("id");

    // The primary key is the database's to assign, so a value read from a file is ignored.
    let named: Vec<(&'static str, &Value)> = columns
        .iter()
        .filter(|(name, _)| name.as_str() != primary_key)
        .map(|(name, value)| checked_column(table, name).map(|column| (column, value)))
        .collect::<Result<_, _>>()?;

    if named.is_empty() {
        // A row with nothing in it is still a row: the database assigns it a key.
        let sql = format!("INSERT INTO {table} DEFAULT VALUES RETURNING {primary_key}");
        return Ok(dispatch!(store, |pool| { sqlx::query_scalar(&sql).fetch_one(pool).await? }));
    }

    let column_list = named.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
    let placeholders = named.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
    let sql = store
        .placeholders(&format!("INSERT INTO {table} ({column_list}) VALUES ({placeholders}) RETURNING {primary_key}"));

    Ok(dispatch!(store, |pool| {
        let mut query = sqlx::query_scalar(&sql);
        for (_, value) in &named {
            query = bind_value!(query, value);
        }
        query.fetch_one(pool).await?
    }))
}

/// The table a C++ model class is stored in, re-exported so importers need not reach into
/// `bt-model` for it.
pub fn table_for(cpp_type: &str) -> Option<&'static str> {
    table_for_cpp_type(cpp_type)
}
