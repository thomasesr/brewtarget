// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/extras.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/recipeExtrasWidget.ui and src/widgets/RecipeExtrasWidget.cpp

//! The Extras tab: who brewed it, how it is conditioned, how it turned out.
//!
//! Fields and labels from `RecipeExtrasWidget`, in its order.

use bt_calc::recipe::Calculated;
use bt_measure::Unit;
use bt_model::recipe::Recipe;
use dioxus::prelude::*;

use super::format::{NOTHING, label, number, percent, text};

/// Energy per serving, from energy per litre.
///
/// The Qt widget shows calories per 33 cl where volumes are metric and per US 12 fl oz where
/// they are not.  Both are unit conversions rather than formulae -- `kilocaloriesPer33cl` and
/// `kilocaloriesPer12us_floz` are in the unit table -- so this converts rather than computes.
fn calories(per_litre: f64, unit_name: &str) -> String {
    match Unit::by_name(unit_name) {
        // The unit table converts *to* the canonical unit, which for energy per volume is
        // kilocalories per litre; a serving is the other direction.
        Some(unit) => format!("{:.0}", unit.from_canonical(per_litre)),
        None => NOTHING.to_owned(),
    }
}

#[component]
pub fn Extras(recipe: Recipe, figures: Calculated) -> Element {
    let row = &recipe.row;

    rsx! {
        dl { class: "bt-facts bt-extras",
            Entry { label: "Brewer", value: text(row.brewer.as_deref()) }
            Entry { label: "Assistant Brewer", value: text(row.assistant_brewer.as_deref()) }
            Entry {
                label: "Taste Rating",
                value: match row.taste_rating {
                    Some(rating) => format!("{rating:.0} / 10"),
                    None => NOTHING.to_owned(),
                },
            }
            Entry {
                label: "Expected wort clarity",
                value: label(row.wort_clarity(), |value| value.display_name()),
            }
            Entry { label: "Bottle/Keg Age (days)", value: number(row.age, 0) }
            Entry { label: "Bottle/Keg Temp", value: number(row.age_temp, 1) }
            Entry {
                label: "Date Created",
                value: row.date.map(|date| date.to_string()).unwrap_or_else(|| NOTHING.to_owned()),
            }
            Entry { label: "Carbonation Volumes", value: number(row.carb_volume, 2) }
            Entry { label: "Final beer pH", value: number(row.beer_acidity_ph, 1) }
            Entry { label: "Total apparent attenuation", value: percent(row.apparent_attenuation_pct) }
            Entry {
                label: "Calories/33cl",
                value: calories(figures.calories_per_liter, "kilocaloriesPer33cl"),
            }
            Entry {
                label: "Calories/12oz",
                value: calories(figures.calories_per_liter, "kilocaloriesPer12us_floz"),
            }
        }

        h3 { class: "bt-group", "Notes" }
        p { class: "bt-prose", "{text(row.notes.as_deref())}" }

        h3 { class: "bt-group", "Taste Notes" }
        p { class: "bt-prose", "{text(row.taste_notes.as_deref())}" }
    }
}

/// One labelled value.
#[component]
fn Entry(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "bt-fact",
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}
