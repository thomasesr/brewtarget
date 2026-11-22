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
pub mod brewday;
pub mod catalogues;
pub mod editor;
pub mod extras;
pub mod format;
pub mod inventory;
pub mod recipe;
pub mod recipes;
pub mod steps;
pub mod table;
pub mod tools;
pub mod transfer;
pub mod water;

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use dioxus::prelude::*;

use crate::route::Route;

/// Which kind of thing a recipe has additions of.
///
/// The four ingredient tabs on the recipe screen; each is a table of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addable {
    Fermentable,
    Hop,
    Misc,
    Yeast,
}

impl Addable {
    pub const ALL: [Self; 4] = [Self::Fermentable, Self::Hop, Self::Misc, Self::Yeast];

    /// The table the additions themselves live in.
    pub fn table(self) -> &'static str {
        match self {
            Self::Fermentable => "fermentable_in_recipe",
            Self::Hop => "hop_in_recipe",
            Self::Misc => "misc_in_recipe",
            Self::Yeast => "yeast_in_recipe",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Fermentable => "Fermentables",
            Self::Hop => "Hops",
            Self::Misc => "Miscellaneous",
            Self::Yeast => "Yeasts",
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Self::Fermentable => "Fermentable",
            Self::Hop => "Hop",
            Self::Misc => "Miscellaneous Ingredient",
            Self::Yeast => "Yeast",
        }
    }

    /// What this tab is called in an address.
    pub fn slug(self) -> &'static str {
        match self {
            Self::Fermentable => "fermentables",
            Self::Hop => "hops",
            Self::Misc => "miscs",
            Self::Yeast => "yeasts",
        }
    }
}

impl fmt::Display for Addable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.slug())
    }
}

/// An address naming a kind of addition that does not exist.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("there is nothing called {0:?} to add to a recipe")]
pub struct NoSuchAddable(String);

impl FromStr for Addable {
    type Err = NoSuchAddable;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL.into_iter().find(|kind| kind.slug() == value).ok_or_else(|| NoSuchAddable(value.to_owned()))
    }
}

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
    /// Every catalogue, in the order the application's own tab strip has them.
    ///
    /// Taken from `tabWidget_Trees` in `ui/mainWindow.ui`: recipes, then the things a recipe
    /// refers to, then the things it is made of.  Not an order worth second-guessing -- it is
    /// what anyone who has used Brewtarget will reach for.
    pub const ALL: [Self; 10] = [
        Self::Styles,
        Self::Equipments,
        Self::Mashes,
        Self::Boils,
        Self::Fermentations,
        Self::Fermentables,
        Self::Hops,
        Self::Miscs,
        Self::Yeasts,
        Self::Waters,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Fermentables => "Fermentables",
            Self::Hops => "Hops",
            Self::Miscs => "Miscs",
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

impl Which {
    /// Which kind of thing this table is stocked as, where it is stocked at all.
    ///
    /// Only the four ingredients a recipe is made of are bought and used up; a style or a mash
    /// profile is not something there is a quantity of.
    pub fn stocked(table: &str) -> Option<&'static str> {
        match table {
            "fermentable" => Some("fermentable"),
            "hop" => Some("hop"),
            "misc" => Some("misc"),
            "yeast" => Some("yeast"),
            _ => None,
        }
    }
}

/// What is on hand of each ingredient, by the ingredient's key.
///
/// `/api/inventory/{kind}` answers with a holding per ingredient; the catalogue wants to look
/// one up by key as it renders each row.
pub fn by_ingredient(holdings: &[serde_json::Map<String, serde_json::Value>]) -> BTreeMap<i32, f64> {
    holdings
        .iter()
        .filter_map(|holding| {
            let id = i32::try_from(holding.get("ingredient_id")?.as_i64()?).ok()?;
            Some((id, holding.get("quantity")?.as_f64()?))
        })
        .collect()
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
                    li { class: "bt-nav-apart",
                        Link {
                            to: Route::Inventory {},
                            class: if matches!(route, Route::Inventory {}) { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                            "Inventory"
                        }
                    }
                    li { class: "bt-nav-apart",
                        span { class: "bt-nav-heading", "Tools" }
                    }
                    for tool in tools::Tool::ALL {
                        li { key: "{tool}",
                            Link {
                                to: Route::Tools { tool },
                                class: if route == (Route::Tools { tool }) { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                                "{tool.title()}"
                            }
                        }
                    }
                    li { class: "bt-nav-apart",
                        Link {
                            to: Route::Transfer {},
                            class: if matches!(route, Route::Transfer {}) { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                            "Import / Export"
                        }
                    }
                }
            }
            section { class: "bt-pane", Outlet::<Route> {} }
        }
    }
}
