// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/store.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The handle on one brewing database.
//!
//! Replaces the `Database` singleton in `src/database/Database.cpp`.  There is no global pool and
//! no ambient "current database": a repository is built from a [`Store`] that the caller passes
//! in, so no code path can reach brewing data without naming which database it means.  That is
//! what keeps Stage 2's per-user stores a change of where the handle comes from rather than a
//! rewrite (PLAN.md §3.3).
//!
//! Each backend gets its own connection pool rather than going through sqlx's `Any` driver, which
//! cannot decode a `DATE` column -- and dates are real columns here, not a corner case.

use bt_schema::{Dialect, Namespace};
use sqlx::{MySqlPool, PgPool, SqlitePool};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0} is not a database URL this build understands (try sqlite:, postgres: or mysql:)")]
    UnknownScheme(String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] bt_schema::MigrationError),
    #[error("{0}")]
    Namespace(#[from] bt_schema::InvalidNamespace),
    #[error(transparent)]
    Ddl(#[from] bt_schema::DdlError),
    #[error("{0} is not one of the folder tables in this schema")]
    UnknownFolderTable(String),
    #[error("{0} is not a table in this schema")]
    UnknownTable(String),
    #[error("{table} has no column called {column}")]
    UnknownColumn { table: String, column: String },
    #[error("could not read {path}: {source}")]
    ContentDirectory { path: String, source: std::io::Error },
    #[error("{path}: {message}")]
    ContentFormat { path: String, message: String },
    #[error("{found} files match {prefix}, expected exactly one")]
    AmbiguousContentFile { prefix: String, found: usize },
    #[error(transparent)]
    Folder(#[from] bt_model::folder::FolderError),
}

/// A connection pool for one brewing database.
#[derive(Debug, Clone)]
pub enum Backend {
    Sqlite(SqlitePool),
    Postgres(PgPool),
    MariaDb(MySqlPool),
}

/// A handle on one brewing database.
///
/// The URL is kept alongside the pool because schema work -- creating tables and running
/// migrations -- goes through sqlx's `Any` driver, which is enough for DDL and keeps `bt-schema`
/// from needing to know about the three backends separately.  Reading and writing rows uses the
/// typed pool.
#[derive(Debug, Clone)]
pub struct Store {
    backend: Backend,
    url: String,
}

/// A single connection taken from a [`Store`], which is what a transaction needs.
///
/// Work that must be atomic -- importing a file, seeding a database -- has to happen on one
/// connection, since `BEGIN` on a pool would scatter statements across several.
#[derive(Debug)]
pub enum Connection {
    Sqlite(sqlx::pool::PoolConnection<sqlx::Sqlite>),
    Postgres(sqlx::pool::PoolConnection<sqlx::Postgres>),
    MariaDb(sqlx::pool::PoolConnection<sqlx::MySql>),
}

/// Run the same code against whichever backend a [`Connection`] is.
///
/// The same trick as [`dispatch`], for a single connection rather than a pool.
#[macro_export]
macro_rules! dispatch_connection {
    ($connection:expr, |$handle:ident| $body:block) => {
        match $connection {
            $crate::Connection::Sqlite($handle) => $body,
            $crate::Connection::Postgres($handle) => $body,
            $crate::Connection::MariaDb($handle) => $body,
        }
    };
}

/// Run the same code against whichever backend a [`Store`] holds.
///
/// Each arm is typechecked separately with its concrete pool type, so one piece of source text
/// serves all three without any of it being generic over `sqlx::Database` -- which would mean
/// spelling out `Encode`/`Type` bounds for every column type we touch.
#[macro_export]
macro_rules! dispatch {
    ($store:expr, |$pool:ident| $body:block) => {
        match $store.backend() {
            $crate::Backend::Sqlite($pool) => $body,
            $crate::Backend::Postgres($pool) => $body,
            $crate::Backend::MariaDb($pool) => $body,
        }
    };
}

impl Store {
    /// Connect to `url`, choosing the backend from its scheme.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let scheme = url.split(':').next().unwrap_or_default();
        let backend = match scheme {
            "sqlite" => Backend::Sqlite(SqlitePool::connect(url).await?),
            "postgres" | "postgresql" => Backend::Postgres(PgPool::connect(url).await?),
            "mysql" | "mariadb" => Backend::MariaDb(MySqlPool::connect(url).await?),
            _ => return Err(StoreError::UnknownScheme(url.to_owned())),
        };
        Ok(Store { backend, url: url.to_owned() })
    }

    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn dialect(&self) -> Dialect {
        match self.backend {
            Backend::Sqlite(_) => Dialect::Sqlite,
            Backend::Postgres(_) => Dialect::Postgresql,
            Backend::MariaDb(_) => Dialect::Mariadb,
        }
    }

    /// Take a single connection, for work that has to happen on one.
    pub async fn connection(&self) -> Result<Connection, StoreError> {
        Ok(match &self.backend {
            Backend::Sqlite(pool) => Connection::Sqlite(pool.acquire().await?),
            Backend::Postgres(pool) => Connection::Postgres(pool.acquire().await?),
            Backend::MariaDb(pool) => Connection::MariaDb(pool.acquire().await?),
        })
    }

    /// Bring a namespace into being, empty.
    ///
    /// Only the namespace: the tables in it are made by connecting *into* it and calling
    /// [`Store::prepare`].  That is the order the eventual design needs -- a connection's
    /// namespace is settled when it is opened, never with a `SET` afterwards, because a pooled
    /// connection told once could later be handed to anything (`PLAN.md` Appendix B).
    ///
    /// SQLite has no namespaces, so there is nothing to make: the file is the namespace.
    pub async fn create_namespace(&self, namespace: &Namespace) -> Result<(), StoreError> {
        if let Some(statement) = bt_schema::create_namespace(self.dialect(), namespace)? {
            let mut connection = self.schema_connection().await?;
            sqlx::query(&statement).execute(&mut connection).await?;
        }
        Ok(())
    }

    /// Remove a namespace and everything in it.
    pub async fn drop_namespace(&self, namespace: &Namespace) -> Result<(), StoreError> {
        if let Some(statement) = bt_schema::drop_namespace(self.dialect(), namespace)? {
            let mut connection = self.schema_connection().await?;
            sqlx::query(&statement).execute(&mut connection).await?;
        }
        Ok(())
    }

    /// A connection for schema work, through the `Any` driver.
    async fn schema_connection(&self) -> Result<sqlx::AnyConnection, StoreError> {
        sqlx::any::install_default_drivers();
        Ok(<sqlx::AnyConnection as sqlx::Connection>::connect(&self.url).await?)
    }

    /// The character that quotes an identifier: a backtick on MariaDB, a double quote elsewhere.
    pub fn identifier_quote(&self) -> &'static str {
        match self.dialect() {
            Dialect::Mariadb => "`",
            Dialect::Sqlite | Dialect::Postgresql => "\"",
        }
    }

    /// Make a statement ready for this backend.
    ///
    /// Two substitutions.  `?` placeholders are numbered on PostgreSQL and left alone elsewhere.
    /// `{q}` becomes the identifier quote, which generated SELECTs use to alias each column back
    /// into the case the schema declares it in -- PostgreSQL folds an unquoted identifier to
    /// lower case, so a column called `sixMonthAlphaLoss_pct` comes back under another name.
    pub fn placeholders(&self, sql: &str) -> String {
        let sql = sql.replace("{q}", self.identifier_quote());
        let sql = &sql;
        bt_schema::migrations::render::bind_placeholders(sql, self.dialect())
    }

    /// Bring the database up to the current schema version, creating it if it is empty.
    ///
    /// Mirrors what `Database::load` does in the C++: a database with no tables is created from
    /// the schema definitions, and one that already exists is migrated.
    pub async fn prepare(&self, namespace: &Namespace) -> Result<u32, StoreError> {
        let mut connection = self.schema_connection().await?;
        if bt_schema::migrations::run::is_empty(&mut connection).await? {
            bt_schema::create_database(&mut connection, self.dialect(), namespace).await?;
            Ok(bt_schema::migrations().latest_version)
        } else {
            Ok(bt_schema::migrate_to_latest(&mut connection, self.dialect()).await?)
        }
    }
}
