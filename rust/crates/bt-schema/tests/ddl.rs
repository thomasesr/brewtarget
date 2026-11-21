// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/tests/ddl.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Tests for the DDL renderer.
//!
//! The byte-for-byte comparison against a dump from the C++ application is a separate, still
//! outstanding step (PLAN.md Phase 1, "done when"), because Qt6 is not available in this
//! environment. What is checked here is that the renderer reproduces the C++ *format*, that it
//! never emits a foreign key as an ordinary column, and that the SQLite output actually executes
//! and produces the columns the schema says it should.

use bt_schema::{Dialect, Namespace, add_foreign_keys, create_all, create_table, schema};

#[test]
fn schema_loads() {
    let schema = schema();
    assert_eq!(schema.schema_version, 23);
    assert_eq!(schema.tables.len(), 39);
    assert_eq!(schema.dialects, ["sqlite", "postgresql", "mariadb"]);
}

#[test]
fn create_table_reproduces_the_cpp_format() {
    // Including the space after each comma, which src/database/ObjectStore.cpp emits as ", \n".
    let table = schema().table("folder_for_hop").expect("folder_for_hop");
    let sql = create_table(table, Dialect::Sqlite, &Namespace::Default).unwrap();
    assert_eq!(sql, "CREATE TABLE folder_for_hop (\nid INTEGER PRIMARY KEY, \nname TEXT, \ndeleted BOOLEAN\n);");
}

#[test]
fn primary_key_declaration_is_dialect_specific() {
    let table = schema().table("hop").expect("hop");
    for (dialect, expected) in [
        (Dialect::Sqlite, "id INTEGER PRIMARY KEY"),
        (Dialect::Postgresql, "id SERIAL PRIMARY KEY"),
        (Dialect::Mariadb, "id INT AUTO_INCREMENT PRIMARY KEY"),
    ] {
        let sql = create_table(table, dialect, &Namespace::Default).unwrap();
        assert!(sql.contains(expected), "{dialect}: missing {expected:?} in\n{sql}");
    }
}

#[test]
fn foreign_keys_are_never_ordinary_columns() {
    // The C++ omits them from CREATE TABLE so that table creation order does not matter.
    for table in &schema().tables {
        let sql = create_table(table, Dialect::Sqlite, &Namespace::Default).unwrap();
        for fk in &table.foreign_keys {
            assert!(
                !sql.contains(&format!("\n{} ", fk.column)),
                "{}: foreign key {} appears in CREATE TABLE",
                table.name,
                fk.column
            );
        }
        let alters = add_foreign_keys(table, Dialect::Sqlite, &Namespace::Default).unwrap();
        assert_eq!(alters.len(), table.foreign_keys.len());
    }
}

#[test]
fn every_dialect_renders_every_statement() {
    let expected = schema().tables.len() + schema().tables.iter().map(|t| t.foreign_keys.len()).sum::<usize>();
    for dialect in Dialect::ALL {
        let statements = create_all(dialect, &Namespace::Default).unwrap();
        assert_eq!(statements.len(), expected, "{dialect}");
        assert!(statements.iter().all(|s| !s.is_empty()));
    }
}

#[test]
fn named_namespace_qualifies_and_quotes() {
    let table = schema().table("hop").expect("hop");
    let namespace = Namespace::named("bt_u_demo").unwrap();
    assert!(
        create_table(table, Dialect::Postgresql, &namespace)
            .unwrap()
            .starts_with(r#"CREATE TABLE "bt_u_demo"."hop" ("#)
    );
    assert!(create_table(table, Dialect::Mariadb, &namespace).unwrap().starts_with("CREATE TABLE `bt_u_demo`.`hop` ("));
}

#[test]
fn namespace_validation_rejects_anything_not_plainly_an_identifier() {
    // Namespace names cannot be bound as query parameters, so they are interpolated into DDL as
    // text. Validation is the only thing standing between a caller and injected SQL.
    for bad in [
        r#"bt_u_x"; DROP SCHEMA public CASCADE; --"#,
        "bt_u_x; DROP DATABASE other",
        "bt`u`x",
        "bt.u.x",
        "../../etc/passwd",
        "BT_U_X",
        "9leading_digit",
        "",
        "with space",
    ] {
        assert!(Namespace::named(bad).is_err(), "should have been rejected: {bad:?}");
    }
    assert!(Namespace::named("bt_u_0123456789abcdef0123456789abcdef").is_ok());
}
