// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/lib.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The Brewtarget interface.
//!
//! Replaces the Qt widgets in `src/` and `ui/`.  The components here are written once and
//! rendered by Dioxus to a browser, a desktop window or a phone, so there is one description of
//! what the application looks like rather than one per platform.
//!
//! Nothing in this crate touches a database or parses a file: it asks `bt-api` over HTTP, which
//! is both the Stage 2 boundary and the reason this builds for `wasm32`.  What it *does* share
//! with the server is `bt-model` and `bt-calc`, so a recipe's figures move as they are edited
//! without a round trip and without the arithmetic being written a second time in another
//! language.

pub mod client;
pub mod views;

pub use client::{ApiError, Client};

use dioxus::prelude::*;

/// The whole application.
#[component]
pub fn App() -> Element {
    // One client for the session, reachable from any component that asks for it.
    use_context_provider(Client::default);

    rsx! {
        document::Link { rel: "stylesheet", href: asset!("/assets/brewtarget.css") }
        main { class: "bt-app",
            header { class: "bt-header",
                h1 { "Brewtarget" }
            }
            views::Catalogue {}
        }
    }
}
