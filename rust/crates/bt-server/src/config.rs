// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-server/src/config.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Where the server gets its settings.
//!
//! All from the environment, with defaults that are safe rather than convenient.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

/// How the server is configured.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where the brewing data lives.
    pub database_url: String,
    /// Where the files that ship with the application are.
    pub data_directory: PathBuf,
    /// Where the built frontend is.
    pub assets_directory: PathBuf,
    pub bind: IpAddr,
    pub port: u16,
    /// Whether to seed an empty database with the content that ships with the application.
    pub load_default_content: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: "sqlite://brewtarget.sqlite".to_owned(),
            data_directory: PathBuf::from("data"),
            // Where the Docker image puts the built interface.  Building it here instead
            // leaves it under `rust/target/dx/bt-web/<profile>/web/public`, which is what
            // `BT_ASSETS_DIR` is for.
            assets_directory: PathBuf::from("assets"),
            // Loopback, deliberately.  Stage 1 has no authentication at all, so anything that can
            // reach the port has full control of the database -- see PLAN.md §3.3 and the
            // security posture in CLAUDE.md.  Binding anywhere else has to be asked for.
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8080,
            load_default_content: true,
        }
    }
}

impl Config {
    /// Read the configuration from the environment.
    pub fn from_environment() -> Self {
        let default = Self::default();
        Self {
            database_url: std::env::var("BT_DB_URL").unwrap_or(default.database_url),
            data_directory: std::env::var("BT_DATA_DIR").map(PathBuf::from).unwrap_or(default.data_directory),
            assets_directory: std::env::var("BT_ASSETS_DIR").map(PathBuf::from).unwrap_or(default.assets_directory),
            bind: std::env::var("BT_BIND").ok().and_then(|value| value.parse().ok()).unwrap_or(default.bind),
            port: std::env::var("BT_PORT").ok().and_then(|value| value.parse().ok()).unwrap_or(default.port),
            load_default_content: std::env::var("BT_LOAD_DEFAULT_CONTENT")
                .map(|value| value != "0" && !value.eq_ignore_ascii_case("false"))
                .unwrap_or(default.load_default_content),
        }
    }

    /// Whether the server will answer to anything beyond this machine.
    pub fn is_exposed(&self) -> bool {
        !self.bind.is_loopback()
    }
}
