// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/mod.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The screens.

pub mod catalogues;
pub mod format;
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

/// The catalogue screen: a list of what there is, and a table of whichever is chosen.
#[component]
pub fn Catalogue() -> Element {
    let mut which = use_signal(|| Which::Fermentables);
    let chosen = which();

    rsx! {
        div { class: "bt-catalogue",
            nav { class: "bt-nav", aria_label: "Catalogues",
                ul {
                    for entry in Which::ALL {
                        li { key: "{entry:?}",
                            button {
                                class: if entry == chosen { "bt-nav-item bt-chosen" } else { "bt-nav-item" },
                                aria_current: if entry == chosen { "page" },
                                onclick: move |_| which.set(entry),
                                "{entry.title()}"
                            }
                        }
                    }
                }
            }
            section { class: "bt-pane", aria_label: "{chosen.title()}",
                h2 { "{chosen.title()}" }
                match chosen {
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
            }
        }
    }
}
