// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-schema/src/migrations/render.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Turn the extracted migration SQL into something a given database will accept.
//!
//! Two things vary by dialect.  Type names and the primary key declaration are left as `{{…}}`
//! placeholders by `tools/extract-migrations.py`, because the C++ interpolates them from
//! `DbNativeVariants`.  Bind placeholders differ too: the extracted SQL uses `?`, which SQLite and
//! MariaDB accept but PostgreSQL does not.

use crate::dialect::Dialect;
use crate::model::schema;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("unknown placeholder {{{{{0}}}}} in migration SQL")]
    UnknownPlaceholder(String),
    #[error("{{{{add_foreign_key}}}} expects 4 arguments, got {0}")]
    ForeignKeyArity(usize),
}

/// Substitute the `{{…}}` placeholders for `dialect`.
pub fn render(sql: &str, dialect: Dialect) -> Result<String, RenderError> {
    let schema = schema();
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let end = rest[start..].find("}}").ok_or_else(|| RenderError::UnknownPlaceholder(rest[start..].into()))?;
        let placeholder = &rest[start + 2..start + end];
        rest = &rest[start + end + 2..];

        let (name, argument) = match placeholder.split_once(':') {
            Some((name, argument)) => (name, Some(argument)),
            None => (placeholder, None),
        };

        match (name, argument) {
            ("type", Some(base)) => {
                let native = schema
                    .native_types
                    .get(base)
                    .and_then(|m| m.get(dialect.key()))
                    .ok_or_else(|| RenderError::UnknownPlaceholder(placeholder.into()))?;
                out.push_str(native);
            }
            ("primary_key", None) => out.push_str(&schema.primary_key_declaration[dialect.key()]),
            ("cascade", None) => out.push_str(cascade(dialect)),
            ("add_foreign_key", Some(arguments)) => {
                let parts: Vec<&str> = arguments.split('|').collect();
                let [table, column, ref_table, ref_column] = parts[..] else {
                    return Err(RenderError::ForeignKeyArity(parts.len()));
                };
                out.push_str(
                    &schema.add_foreign_key[dialect.key()]
                        .replace("{table}", table)
                        .replace("{column}", column)
                        .replace("{ref_table}", ref_table)
                        .replace("{ref_column}", ref_column),
                );
            }
            _ => return Err(RenderError::UnknownPlaceholder(placeholder.into())),
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// `DbNativeVariants const cascade` in `src/database/Database.cpp`: empty for SQLite.
fn cascade(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::Sqlite => "",
        Dialect::Postgresql | Dialect::Mariadb => "CASCADE",
    }
}

/// Rewrite `?` bind placeholders as `$1`, `$2`, … for PostgreSQL.
///
/// Question marks inside SQL string literals are left alone; `'Bt:%'`-style literals appear
/// throughout these migrations.
pub fn bind_placeholders(sql: &str, dialect: Dialect) -> String {
    rewrite_placeholders(sql, dialect, &[]).0
}

/// Rewrite the bind placeholders, inlining any that are better written as SQL.
///
/// `QDate::currentDate()` is the only runtime value the migrations bind, and binding it is
/// awkward: it would have to travel as text, and PostgreSQL will not accept text for a `date`
/// column parameter. `CURRENT_DATE` means the same thing and is valid in all three dialects, so
/// it is written into the statement and dropped from the bind list.
///
/// Returns the rewritten SQL and the indices of the binds that are still to be supplied.
pub fn rewrite_placeholders(sql: &str, dialect: Dialect, inline: &[usize]) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(sql.len());
    let mut in_literal = false;
    let (mut seen, mut numbered, mut remaining) = (0usize, 0usize, Vec::new());
    for character in sql.chars() {
        match character {
            '\'' => {
                in_literal = !in_literal;
                out.push(character);
            }
            '?' if !in_literal => {
                if inline.contains(&seen) {
                    out.push_str("CURRENT_DATE");
                } else {
                    remaining.push(seen);
                    numbered += 1;
                    if dialect == Dialect::Postgresql {
                        out.push_str(&format!("${numbered}"));
                    } else {
                        out.push('?');
                    }
                }
                seen += 1;
            }
            _ => out.push(character),
        }
    }
    (out, remaining)
}
