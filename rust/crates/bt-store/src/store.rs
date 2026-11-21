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
    #[error("{0} is not one of the folder tables in this schema")]
    UnknownFolderTable(String),
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

    /// A connection for schema work, through the `Any` driver.
    async fn schema_connection(&self) -> Result<sqlx::AnyConnection, StoreError> {
        sqlx::any::install_default_drivers();
        Ok(<sqlx::AnyConnection as sqlx::Connection>::connect(&self.url).await?)
    }

    /// Rewrite `?` placeholders for the backend: PostgreSQL numbers them, the others do not.
    pub fn placeholders(&self, sql: &str) -> String {
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
