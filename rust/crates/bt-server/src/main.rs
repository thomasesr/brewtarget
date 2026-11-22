// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-server/src/main.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The Brewtarget web server.
//!
//! One process: it serves the JSON API and the built frontend, and brings the database up to the
//! current schema on the way in.

mod config;

use std::net::SocketAddr;

use axum::Router;
use bt_api::{AppState, api};
use bt_schema::Namespace;
use bt_store::Store;
use config::Config;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = Config::from_environment();
    warn_if_exposed(&config);

    let store = Store::connect(&config.database_url).await?;
    let version = store.prepare(&Namespace::Default).await?;
    tracing::info!(schema_version = version, url = %redacted(&config.database_url), "database ready");

    if config.load_default_content {
        match bt_store::default_content::load(&store, &config.data_directory).await {
            Ok(bt_store::default_content::Loaded::NothingToDo { version }) => {
                tracing::info!(content_version = version, "default content already loaded");
            }
            Ok(bt_store::default_content::Loaded::UpTo { version, files, rows_written }) => {
                tracing::info!(content_version = version, files = files.len(), rows_written, "loaded default content");
            }
            // A database that will not seed is still a database that works, so this is reported
            // rather than fatal.
            Err(error) => tracing::warn!(%error, "could not load the default content"),
        }
    }

    let state = AppState::new(store, &config.data_directory);
    let app = api().with_state(state).merge(frontend(&config)).layer(TraceLayer::new_for_http());

    let address = SocketAddr::new(config.bind, config.port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(%address, "listening");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Serve the built frontend, falling back to its index for anything the API did not answer.
///
/// A single-page application routes in the browser, so a request for a path the server has never
/// heard of is still a page rather than a mistake.
fn frontend(config: &Config) -> Router {
    let index = config.assets_directory.join("index.html");
    Router::new().fallback_service(ServeDir::new(&config.assets_directory).fallback(ServeFile::new(index)))
}

/// Say loudly when the server is reachable from beyond this machine.
///
/// Stage 1 has no authentication: whoever can reach the port can read, change or delete
/// everything.  Binding to anything but loopback is a deliberate act and is worth saying so.
fn warn_if_exposed(config: &Config) {
    if config.is_exposed() {
        tracing::warn!(
            bind = %config.bind,
            "this build has NO AUTHENTICATION: anyone who can reach this address has full control \
             of the database, including deleting it. Do not run it on a shared or public network."
        );
    }
}

/// A database URL with any password taken out, for logging.
fn redacted(url: &str) -> String {
    // Everything between "://" and the "@" is credentials.
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    match rest.split_once('@') {
        Some((_, host)) => format!("{scheme}://***@{host}"),
        None => url.to_owned(),
    }
}
