// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/tests/postgres_execution.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Execute the generated PostgreSQL DDL against a real server.
//!
//! Skipped unless `BT_TEST_POSTGRES_URL` is set, so neither CI nor a developer machine needs a
//! PostgreSQL server to run `cargo test`.
//!
//! The whole test runs inside one transaction which is always rolled back. PostgreSQL has
//! transactional DDL, so the schema, its tables and its constraints never become visible to
//! anything else and nothing is left behind on the server -- it is safe to point this at an
//! existing development database.

use std::collections::BTreeSet;

use bt_schema::{Dialect, Namespace, create_all, schema};
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;

const TEST_NAMESPACE: &str = "bt_schema_rendering_test";

fn wanted_columns(table: &bt_schema::Table) -> BTreeSet<String> {
    let mut wanted = BTreeSet::new();
    wanted.insert(table.primary_key.name.clone());
    wanted.extend(table.columns.iter().map(|c| c.name.clone()));
    wanted.extend(table.foreign_keys.iter().map(|f| f.column.clone()));
    wanted
}

#[tokio::test]
async fn generated_postgres_ddl_builds_the_expected_schema() {
    let Ok(url) = std::env::var("BT_TEST_POSTGRES_URL") else {
        eprintln!("skipping: BT_TEST_POSTGRES_URL is not set");
        return;
    };

    let pool = PgPoolOptions::new().max_connections(1).connect(&url).await.expect("connect");
    let mut tx = pool.begin().await.expect("begin");

    let namespace = Namespace::named(TEST_NAMESPACE).unwrap();
    sqlx::query(&format!("CREATE SCHEMA \"{TEST_NAMESPACE}\"")).execute(&mut *tx).await.expect("create schema");

    for statement in create_all(Dialect::Postgresql, &namespace).unwrap() {
        sqlx::query(&statement)
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|e| panic!("failed to execute:\n{statement}\n{e}"));
    }

    let created: BTreeSet<String> =
        sqlx::query("SELECT table_name FROM information_schema.tables WHERE table_schema = $1")
            .bind(TEST_NAMESPACE)
            .fetch_all(&mut *tx)
            .await
            .expect("list tables")
            .into_iter()
            .map(|row| row.get::<String, _>("table_name"))
            .collect();
    assert_eq!(created, schema().tables.iter().map(|t| t.name.clone()).collect::<BTreeSet<_>>());

    for table in &schema().tables {
        let actual: BTreeSet<String> = sqlx::query(
            "SELECT column_name FROM information_schema.columns WHERE table_schema = $1 AND table_name = $2",
        )
        .bind(TEST_NAMESPACE)
        .bind(&table.name)
        .fetch_all(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("columns of {}: {e}", table.name))
        .into_iter()
        .map(|row| row.get::<String, _>("column_name"))
        .collect();
        assert_eq!(actual, wanted_columns(table), "columns of {}", table.name);
    }

    // Explicit, though dropping the transaction would also roll it back.
    tx.rollback().await.expect("rollback");
}
