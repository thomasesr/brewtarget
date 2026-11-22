// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/mod.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The screens.

pub mod additions;
pub mod catalogues;
pub mod editor;
pub mod format;
pub mod recipe;
pub mod recipes;
pub mod table;

use std::fmt;
use std::str::FromStr;

use dioxus::prelude::*;

use crate::route::Route;

/// Which catalogue is being looked at.
///
/// Part of the address, so it has to survive being written down and read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Fermentables,
    Hops,
    Miscs,
    Yeasts,
    Waters,
    Styles,
    Equipments,
    Mashes,
    Boils,
    Fermentations,
}

impl Which {
    /// Every catalogue, in the order the Qt tabs have them: the four ingredients a recipe is
    /// made of, then the things it is made with.
    pub const ALL: [Self; 10] = [
        Self::Fermentables,
        Self::Hops,
        Self::Miscs,
        Self::Yeasts,
        Self::Waters,
        Self::Styles,
        Self::Equipments,
        Self::Mashes,
        Self::Boils,
        Self::Fermentations,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Fermentables => "Fermentables",
            Self::Hops => "Hops",
            Self::Miscs => "Miscellaneous",
            Self::Yeasts => "Yeasts",
            Self::Waters => "Water",
            Self::Styles => "Styles",
            Self::Equipments => "Equipment",
            Self::Mashes => "Mash Profiles",
            Self::Boils => "Boil Profiles",
            Self::Fermentations => "Fermentation Profiles",
        }
    }

    /// The table this catalogue lists.
    pub fn table(self) -> &'static str {
        match self {
            Self::Fermentables => "fermentable",
            Self::Hops => "hop",
            Self::Miscs => "misc",
            Self::Yeasts => "yeast",
            Self::Waters => "water",
            Self::Styles => "style",
            Self::Equipments => "equipment",
            Self::Mashes => "mash",
            Self::Boils => "boil",
            Self::Fermentations => "fermentation",
        }
    }

    /// What one of these is called on its own, for the heading over its editor.
    pub fn singular(self) -> &'static str {
        match self {
            Self::Fermentables => "Fermentable",
            Self::Hops => "Hop",
            Self::Miscs => "Miscellaneous Ingredient",
            Self::Yeasts => "Yeast",
            Self::Waters => "Water",
            Self::Styles => "Style",
            Self::Equipments => "Equipment",
            Self::Mashes => "Mash Profile",
            Self::Boils => "Boil Profile",
            Self::Fermentations => "Fermentation Profile",
        }
    }

    /// What this catalogue is called in an address.  Lower case and stable: these end up in
    /// links people keep, so they are not to be renamed lightly.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Fermentables => "fermentables",
            Self::Hops => "hops",
            Self::Miscs => "miscs",
            Self::Yeasts => "yeasts",
            Self::Waters => "water",
            Self::Styles => "styles",
            Self::Equipments => "equipment",
            Self::Mashes => "mashes",
            Self::Boils => "boils",
            Self::Fermentations => "fermentations",
        }
    }
}

impl fmt::Display for Which {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.slug())
    }
}

/// An address naming a catalogue that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("there is no catalogue called {0:?}")]
pub struct NoSuchCatalogue(String);

impl FromStr for Which {
    type Err = NoSuchCatalogue;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|which| which.slug() == value).ok_or_else(|| NoSuchCatalogue(value.to_owned()))
    }
}

/// The frame every screen sits in: the title, the list of sections, and whichever screen the
/// address names.
#[component]
pub fn Shell() -> Element {
    let route = use_route::<Route>();

    // A recipe being looked at still belongs under Recipes.
    let on_recipes = matches!(route, Route::Home {} | Route::Recipes {} | Route::Recipe { .. });
    let open_catalogue = match route {
        Route::Catalogue { which } => Some(which),
        _ => None,
    };

    rsx! {
        header { class: "bt-header",
            h1 {
                Link { to: Route::Recipes {}, class: "bt-title", "Brewtarget" }
            }
        }
        div { class: "bt-catalogue",
            nav { class: "bt-nav", aria_label: "Sections",
                ul {
                    li {
                        Link {
                            to: Route::Recipes {},
                            class: if on_recipes { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                            "Recipes"
                        }
                    }
                    for which in Which::ALL {
                        li { key: "{which}",
                            Link {
                                to: Route::Catalogue { which },
                                class: if open_catalogue == Some(which) { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                                "{which.title()}"
                            }
                        }
                    }
                }
            }
            section { class: "bt-pane", Outlet::<Route> {} }
        }
    }
}
