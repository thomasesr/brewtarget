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
pub mod format;
pub mod recipe;
pub mod recipes;
pub mod table;

use dioxus::prelude::*;

/// Which catalogue is being looked at.
///
/// Replaces the left-hand tree in `ui/mainWindow.ui`, which is a tree because Qt gave it one for
/// free; these are a flat list of ten things, so they are shown as one.
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
    const ALL: [Self; 10] = [
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

    fn title(self) -> &'static str {
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
}

/// What is being looked at.
///
/// Held in one place so that the day this gains real URLs, a route maps onto one of these and
/// nothing else has to change.  Replaces the left-hand tree in `ui/mainWindow.ui` and the stack
/// of tabs beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Every recipe there is.
    Recipes,
    /// One recipe, by its key.
    Recipe(i32),
    /// One of the catalogues.
    Catalogue(Which),
}

impl Screen {
    fn title(self) -> String {
        match self {
            Self::Recipes | Self::Recipe(_) => "Recipes".to_owned(),
            Self::Catalogue(which) => which.title().to_owned(),
        }
    }

    /// Whether the nav entry for this screen should read as the current one.  A recipe being
    /// looked at still belongs to Recipes.
    fn is(self, entry: Self) -> bool {
        matches!((self, entry), (Self::Recipe(_) | Self::Recipes, Self::Recipes)) || self == entry
    }
}

/// Where the interface can be asked to go.
pub type Navigation = Signal<Screen>;

/// The main screen: a list of what there is, and whatever is chosen from it.
#[component]
pub fn Catalogue() -> Element {
    let mut screen = use_context_provider(|| Signal::new(Screen::Recipes));
    let chosen = screen();

    let entries: Vec<Screen> = std::iter::once(Screen::Recipes).chain(Which::ALL.map(Screen::Catalogue)).collect();

    rsx! {
        div { class: "bt-catalogue",
            nav { class: "bt-nav", aria_label: "Sections",
                ul {
                    for entry in entries {
                        li { key: "{entry:?}",
                            button {
                                class: if chosen.is(entry) { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                                aria_current: if chosen.is(entry) { "page" },
                                onclick: move |_| screen.set(entry),
                                "{entry.title()}"
                            }
                        }
                    }
                }
            }
            section { class: "bt-pane", aria_label: "{chosen.title()}",
                match chosen {
                    Screen::Recipes => rsx! {
                        h2 { "Recipes" }
                        recipes::Recipes {}
                    },
                    Screen::Recipe(id) => rsx! { recipe::RecipeView { id } },
                    Screen::Catalogue(which) => rsx! {
                        h2 { "{which.title()}" }
                        match which {
                            Which::Fermentables => rsx! { catalogues::Fermentables {} },
                            Which::Hops => rsx! { catalogues::Hops {} },
                            Which::Miscs => rsx! { catalogues::Miscs {} },
                            Which::Yeasts => rsx! { catalogues::Yeasts {} },
                            Which::Waters => rsx! { catalogues::Waters {} },
                            Which::Styles => rsx! { catalogues::Styles {} },
                            Which::Equipments => rsx! { catalogues::Equipments {} },
                            Which::Mashes => rsx! { catalogues::Mashes {} },
                            Which::Boils => rsx! { catalogues::Boils {} },
                            Which::Fermentations => rsx! { catalogues::Fermentations {} },
                        }
                    },
                }
            }
        }
    }
}
