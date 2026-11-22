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
    // The instructions the recipe carries, or the ones it implies.  The Qt application writes
    // these from the recipe on demand; a recipe read from a file usually has none of its own.
    let stored: Vec<(i32, String, String)> = recipe
        .instructions
        .iter()
        .filter(|step| !step.deleted.unwrap_or(false))
        .map(|step| {
            (
                step.step_number.unwrap_or_default(),
                step.name.clone().unwrap_or_default(),
                step.directions.clone().unwrap_or_default(),
            )
        })
        .collect();
    let generated = stored.is_empty();
    let instructions: Vec<(i32, String, String)> = if generated {
        bt_calc::instructions::generate(&recipe)
            .into_iter()
            .map(|step| (step.step_number, step.name, step.directions))
            .collect()
    } else {
        stored
    };

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
            p { class: "bt-empty", "There is nothing to do: this recipe has no mash, boil or fermentation." }
        } else {
            if generated {
                p { class: "bt-note",
                    "Written from the recipe.  The recipe carries no instructions of its own, so
                     these follow from its mash, boil and additions."
                }
            }
            ol { class: "bt-steps",
                for (number, name, directions) in instructions.iter() {
                    li { key: "{number}",
                        strong { "{text(Some(name.as_str()))}" }
                        p { class: "bt-prose", "{text(Some(directions.as_str()))}" }
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
