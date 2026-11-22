// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/recipe.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/mainWindow.ui and src/qtModels/tableModels/RecipeAddition*TableModel.cpp

//! One recipe: what it is, what it comes to, and what goes into it.
//!
//! The figures are worked out here, by `bt_calc`, rather than asked of the server.  That is the
//! whole reason this interface is Rust: the same code the mirrored C++ tests cover runs in the
//! browser, so the panel can follow an edit without a round trip and without the arithmetic
//! existing twice.

use bt_calc::recipe::Calculated;
use bt_model::recipe::Recipe;
use dioxus::prelude::*;

use super::additions;
use super::format::{NOTHING, number, percent, text};
use super::table::{Failed, Loading};
use super::{Navigation, Screen};
use crate::Client;

/// Which ingredient tab is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Fermentables,
    Hops,
    Miscs,
    Yeasts,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Fermentables, Self::Hops, Self::Miscs, Self::Yeasts];

    fn title(self) -> &'static str {
        match self {
            Self::Fermentables => "Fermentables",
            Self::Hops => "Hops",
            Self::Miscs => "Miscellaneous",
            Self::Yeasts => "Yeasts",
        }
    }
}

#[component]
pub fn RecipeView(id: i32) -> Element {
    let client = use_context::<Client>();
    let recipe = use_resource(move || {
        let client = client.clone();
        async move { client.recipe(id).await }
    });

    match &*recipe.read_unchecked() {
        None => rsx! { Loading {} },
        Some(Err(error)) => rsx! { Failed { message: error.to_string() } },
        Some(Ok(recipe)) => rsx! { Loaded { recipe: recipe.clone() } },
    }
}

/// A recipe that has arrived.
#[component]
fn Loaded(recipe: Recipe) -> Element {
    let mut screen = use_context::<Navigation>();
    let mut tab = use_signal(|| Tab::Fermentables);
    let open = tab();

    // Worked out in the browser, through the same entry point the server uses, so that the two
    // agree on an untouched recipe and this one keeps up with an edited one.
    let figures = bt_calc::recipe::calculate_recipe(&recipe, Default::default(), Default::default());

    rsx! {
        header { class: "bt-recipe-header",
            button { class: "bt-link bt-back", onclick: move |_| screen.set(Screen::Recipes), "← Recipes" }
            h2 { "{text(recipe.row.name.as_deref())}" }
            dl { class: "bt-facts",
                Fact { label: "Brewer", value: text(recipe.row.brewer.as_deref()) }
                Fact { label: "Style", value: text(recipe.style.as_ref().and_then(|style| style.name.as_deref())) }
                Fact { label: "Equipment", value: text(recipe.equipment.as_ref().and_then(|kit| kit.name.as_deref())) }
                Fact { label: "Batch", value: litres(recipe.row.batch_size) }
                Fact { label: "Boil", value: minutes(bt_calc::recipe::boil_time_mins(&recipe)) }
                Fact { label: "Efficiency", value: percent(recipe.row.efficiency) }
            }
        }

        Derived { figures: figures.clone() }

        div { class: "bt-tabs", role: "tablist",
            for entry in Tab::ALL {
                button {
                    key: "{entry:?}",
                    role: "tab",
                    class: if entry == open { "bt-tab bt-chosen" } else { "bt-tab" },
                    aria_selected: "{entry == open}",
                    onclick: move |_| tab.set(entry),
                    "{entry.title()}"
                }
            }
        }
        div { role: "tabpanel", aria_label: "{open.title()}",
            match open {
                Tab::Fermentables => rsx! { additions::Fermentables { recipe: recipe.clone() } },
                Tab::Hops => rsx! { additions::Hops { recipe: recipe.clone(), figures: figures.clone() } },
                Tab::Miscs => rsx! { additions::Miscs { recipe: recipe.clone() } },
                Tab::Yeasts => rsx! { additions::Yeasts { recipe: recipe.clone() } },
            }
        }
    }
}

/// One labelled fact in the recipe header.
#[component]
fn Fact(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "bt-fact",
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}

/// The panel of figures the Qt recipe tab shows across the top.
#[component]
fn Derived(figures: Calculated) -> Element {
    rsx! {
        dl { class: "bt-derived",
            Figure { label: "OG", value: format!("{:.3}", figures.og) }
            Figure { label: "FG", value: format!("{:.3}", figures.fg) }
            Figure { label: "ABV", value: format!("{:.1}%", figures.abv_pct) }
            Figure { label: "IBU", value: format!("{:.0}", figures.ibus) }
            Figure { label: "Colour", value: format!("{:.1} SRM", figures.color_srm) }
            Figure { label: "Calories", value: format!("{:.0} /L", figures.calories_per_liter) }
        }
    }
}

#[component]
fn Figure(label: &'static str, value: String) -> Element {
    rsx! {
        div { class: "bt-figure",
            dt { "{label}" }
            dd { "{value}" }
        }
    }
}

/// A volume in litres.
fn litres(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{} L", number(Some(value), 1)),
        None => NOTHING.to_owned(),
    }
}

/// A length of time in minutes.
fn minutes(value: f64) -> String {
    format!("{value:.0} min")
}
