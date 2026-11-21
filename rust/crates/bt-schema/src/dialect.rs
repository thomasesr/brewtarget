//! SQL dialects and the namespace a schema is rendered into.
//!
//! Mirrors `DbNativeVariants` in `src/database/Database.cpp`, which covers SQLite and PostgreSQL.
//! MariaDB is a third variant, new in this port.

use std::fmt;

/// Which database the DDL is being rendered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    Sqlite,
    Postgresql,
    Mariadb,
}

impl Dialect {
    pub const ALL: [Dialect; 3] = [Dialect::Sqlite, Dialect::Postgresql, Dialect::Mariadb];

    /// The key this dialect is stored under in `schema/v23.json`.
    pub fn key(self) -> &'static str {
        match self {
            Dialect::Sqlite => "sqlite",
            Dialect::Postgresql => "postgresql",
            Dialect::Mariadb => "mariadb",
        }
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// Where the tables live.
///
/// Stage 1 always renders into [`Namespace::Default`], which produces bare, unquoted identifiers
/// byte-identical to what the C++ emits. The parameter exists because Stage 2 gives each user a
/// private namespace — a PostgreSQL schema or a MariaDB database — and nothing in the renderer
/// may hardcode `public` or a fixed database name (see PLAN.md §3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Namespace {
    /// The connection's default namespace: the SQLite file, PostgreSQL's `search_path`, or the
    /// MariaDB connection's default database.
    Default,
    /// An explicitly named namespace, qualifying and quoting every identifier.
    Named(String),
}

/// A namespace name that failed validation.
///
/// Namespace names cannot be bound as query parameters, so they reach SQL as interpolated text.
/// This is the only place a caller-supplied value does so, and it is validated here, at the single
/// point where that interpolation happens.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid namespace name {0:?}: expected 1-63 characters matching ^[a-z][a-z0-9_]*$")]
pub struct InvalidNamespace(pub String);

impl Namespace {
    /// Validate a namespace name.
    ///
    /// Deliberately strict: lower case ASCII letters, digits and underscores only, starting with a
    /// letter. This rejects quotes, semicolons, whitespace, dots and path separators outright
    /// rather than trying to escape them.
    pub fn named(name: impl Into<String>) -> Result<Self, InvalidNamespace> {
        let name = name.into();
        let valid = (1..=63).contains(&name.len())
            && name.starts_with(|c: char| c.is_ascii_lowercase())
            && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if valid { Ok(Namespace::Named(name)) } else { Err(InvalidNamespace(name)) }
    }

    /// Render `table`, qualified by this namespace and quoted with `quote`.
    ///
    /// [`Namespace::Default`] renders the bare table name, exactly as the C++ does.
    pub(crate) fn qualify(&self, table: &str, quote: &str) -> String {
        match self {
            Namespace::Default => table.to_owned(),
            Namespace::Named(ns) => format!("{quote}{ns}{quote}.{quote}{table}{quote}"),
        }
    }
}
