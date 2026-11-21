// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/examples/dump-ddl.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Print the whole schema as DDL for one dialect.
//!
//! Used to diff generated output against a dump taken from the C++ application (PLAN.md Phase 1).
//!
//!     cargo run -p bt-schema --example dump-ddl -- sqlite [namespace]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dialect = match args.next().as_deref() {
        Some("sqlite") => bt_schema::Dialect::Sqlite,
        Some("postgresql") => bt_schema::Dialect::Postgresql,
        Some("mariadb") => bt_schema::Dialect::Mariadb,
        other => return Err(format!("expected sqlite|postgresql|mariadb, got {other:?}").into()),
    };
    let namespace = match args.next() {
        Some(name) => bt_schema::Namespace::named(name)?,
        None => bt_schema::Namespace::Default,
    };
    for statement in bt_schema::create_all(dialect, &namespace)? {
        println!("{statement}");
    }
    Ok(())
}
