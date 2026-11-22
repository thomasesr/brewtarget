// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/route.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Where everything is.
//!
//! A desktop application does not need addresses, which is why the Qt version has none.  A page
//! does: a recipe someone is looking at should survive being reloaded, be sendable to someone
//! else, and come back when the Back button is pressed.
//!
//! Every screen is reachable by its own path, so the interface has no navigation state of its
//! own -- the address *is* the state.

use dioxus::prelude::*;

use crate::views;

/// Every screen, and the address it lives at.
#[derive(Debug, Clone, PartialEq, Eq, Routable)]
#[rustfmt::skip]
pub enum Route {
    #[layout(views::Shell)]
        #[route("/")]
        Home {},
        #[route("/recipes")]
        Recipes {},
        #[route("/recipes/:id")]
        Recipe { id: i32 },
        #[route("/transfer")]
        Transfer {},
        #[route("/catalogue/:which")]
        Catalogue { which: views::Which },
        #[route("/catalogue/:which/new")]
        NewEntry { which: views::Which },
        #[route("/catalogue/:which/:id")]
        EditEntry { which: views::Which, id: i32 },
    // Anything else is a mistyped or outdated address, which is worth saying rather than
    // showing an empty screen.
    #[route("/:..segments")]
    Missing { segments: Vec<String> },
}

/// The address someone arrives at with nothing more specific in mind.
#[component]
pub fn Home() -> Element {
    rsx! { views::recipes::Recipes {} }
}

#[component]
pub fn Recipes() -> Element {
    rsx! { views::recipes::Recipes {} }
}

#[component]
pub fn Recipe(id: i32) -> Element {
    rsx! { views::recipe::RecipeView { id } }
}

#[component]
pub fn Catalogue(which: views::Which) -> Element {
    rsx! { views::catalogues::Catalogue { which } }
}

#[component]
pub fn Transfer() -> Element {
    rsx! { views::transfer::Transfer {} }
}

#[component]
pub fn NewEntry(which: views::Which) -> Element {
    rsx! { views::editor::Screen { which, editing: views::editor::Editing::New } }
}

#[component]
pub fn EditEntry(which: views::Which, id: i32) -> Element {
    rsx! { views::editor::Screen { which, editing: views::editor::Editing::Existing(id) } }
}

/// An address that names nothing.
#[component]
pub fn Missing(segments: Vec<String>) -> Element {
    let path = segments.join("/");
    rsx! {
        p { class: "bt-error", role: "alert", "There is nothing at /{path}." }
        Link { to: Route::Recipes {}, class: "bt-link", "Back to the recipes" }
    }
}
