// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/brewday.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/brewDayScrollWidget.ui and src/BrewDayScrollWidget.cpp

//! The Brewday tab: the sheet you brew from.
//!
//! The figures the Qt widget prints at the top, then the recipe'"'"'s instructions in order.
//! Printing is the browser'"'"'s job rather than a dialog of its own, so this is laid out to print
//! as well as it reads.

use bt_model::recipe::Recipe;
use dioxus::prelude::*;

use super::format::{number, text};
use bt_calc::recipe::Calculated;

#[component]
pub fn Brewday(recipe: Recipe, figures: Calculated) -> Element {
    let instructions: Vec<_> =
        recipe.instructions.iter().filter(|step| !step.deleted.unwrap_or(false)).cloned().collect();

    rsx! {
        dl { class: "bt-facts",
            Entry { label: "Boil Volume", value: litres(figures.boil_volume_l) }
            Entry { label: "Post-Boil Volume", value: litres(figures.post_boil_volume_l) }
            Entry { label: "Into Fermenter", value: litres(figures.final_volume_l) }
            Entry { label: "Boil Time", value: format!("{:.0} min", bt_calc::recipe::boil_time_mins(&recipe)) }
            Entry { label: "Grain", value: format!("{:.3} kg", figures.grains_kg) }
            Entry { label: "Wort From Mash", value: litres(figures.wort_from_mash_l) }
        }

        h3 { class: "bt-group", "Steps" }
        if instructions.is_empty() {
            p { class: "bt-empty",
                "This recipe has no instructions.  The Qt application writes them from the recipe
                 on demand; that is not ported yet, so a recipe only has instructions if the file
                 it came from carried them."
            }
        } else {
            ol { class: "bt-steps",
                for step in instructions.iter() {
                    li { key: "{step.id}",
                        strong { "{text(step.name.as_deref())}" }
                        p { class: "bt-prose", "{text(step.directions.as_deref())}" }
                    }
                }
            }
        }
    }
}

fn litres(value: f64) -> String {
    format!("{} L", number(Some(value), 2))
}

#[component]
fn Entry(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "bt-fact",
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}
