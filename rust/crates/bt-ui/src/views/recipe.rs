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

use super::format::{NOTHING, number, percent, text};
use super::table::{Failed, Loading};
use super::{additions, brewday, extras, steps, water};
use crate::Client;
use crate::route::Route;

/// The four tabs above the recipe's ingredients.
///
/// `tabWidget_recipeView` in `ui/mainWindow.ui`: the recipe itself, the details that are not
/// part of brewing it, its water, and the sheet to brew from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Recipe,
    Extras,
    Water,
    Brewday,
}

impl View {
    const ALL: [Self; 4] = [Self::Recipe, Self::Extras, Self::Water, Self::Brewday];

    fn title(self) -> &'static str {
        match self {
            Self::Recipe => "Recipe",
            Self::Extras => "Extras",
            Self::Water => "Water",
            Self::Brewday => "Brewday",
        }
    }
}

/// Which of the recipe's tabs is open.
///
/// The seven `tabWidget_ingredients` has in `ui/mainWindow.ui`, in its order: what goes into
/// the recipe, then what is done to it.  The last three are not ingredients at all, which is
/// why the Qt application puts them here rather than with the profiles -- they are the steps
/// of *this* recipe's mash, boil and fermentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Fermentables,
    Hops,
    Miscs,
    Yeasts,
    Mash,
    Boil,
    Fermentation,
}

impl Tab {
    const ALL: [Self; 7] =
        [Self::Fermentables, Self::Hops, Self::Miscs, Self::Yeasts, Self::Mash, Self::Boil, Self::Fermentation];

    fn title(self) -> &'static str {
        match self {
            Self::Fermentables => "Fermentables",
            Self::Hops => "Hops",
            Self::Miscs => "Miscellaneous",
            Self::Yeasts => "Yeast",
            Self::Mash => "Mash",
            Self::Boil => "Boil",
            Self::Fermentation => "Fermentation",
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
    let mut view = use_signal(|| View::Recipe);
    let open_view = view();
    let mut tab = use_signal(|| Tab::Fermentables);
    let open = tab();

    // Worked out in the browser, through the same entry point the server uses, so that the two
    // agree on an untouched recipe and this one keeps up with an edited one.
    let figures = bt_calc::recipe::calculate_recipe(&recipe, Default::default(), Default::default());

    rsx! {
        header { class: "bt-recipe-header",
            Link { to: Route::Recipes {}, class: "bt-link bt-back", "← Recipes" }
            h2 { "{text(recipe.row.name.as_deref())}" }
            // The fields the Recipe tab of `ui/mainWindow.ui` shows, in its order.  "Target"
            // and "Calculated" sit under a "Batch Size" heading there, which is why two
            // volumes can share such short names without being ambiguous.
            dl { class: "bt-facts",
                Fact { label: "Style", value: text(recipe.style.as_ref().and_then(|style| style.name.as_deref())) }
                Fact { label: "Equipment", value: text(recipe.equipment.as_ref().and_then(|kit| kit.name.as_deref())) }
                Fact { label: "Batch Size — Target", value: litres(recipe.row.batch_size) }
                Fact { label: "Batch Size — Calculated", value: litres(Some(figures.final_volume_l)) }
                Fact { label: "Efficiency", value: percent(recipe.row.efficiency) }
                Fact {
                    label: "Pre-Boil Volume",
                    value: litres(recipe.boil.as_ref().and_then(|(boil, _)| boil.pre_boil_size_l)),
                }
                Fact { label: "Boil Time", value: minutes(bt_calc::recipe::boil_time_mins(&recipe)) }
                Fact { label: "Brewer", value: text(recipe.row.brewer.as_deref()) }
            }
        }

        Derived { figures: figures.clone() }

        div { class: "bt-tabs", role: "tablist",
            for entry in View::ALL {
                button {
                    key: "{entry:?}",
                    role: "tab",
                    class: if entry == open_view { "bt-tab bt-chosen" } else { "bt-tab" },
                    aria_selected: "{entry == open_view}",
                    onclick: move |_| view.set(entry),
                    "{entry.title()}"
                }
            }
        }

        div { role: "tabpanel", aria_label: "{open_view.title()}",
            match open_view {
                View::Recipe => rsx! {
                    div { class: "bt-tabs bt-subtabs", role: "tablist",
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
                            Tab::Hops => rsx! {
                                additions::Hops { recipe: recipe.clone(), figures: figures.clone() }
                            },
                            Tab::Miscs => rsx! { additions::Miscs { recipe: recipe.clone() } },
                            Tab::Yeasts => rsx! { additions::Yeasts { recipe: recipe.clone() } },
                            Tab::Mash => rsx! { steps::MashSteps { recipe: recipe.clone() } },
                            Tab::Boil => rsx! { steps::BoilSteps { recipe: recipe.clone() } },
                            Tab::Fermentation => rsx! { steps::FermentationSteps { recipe: recipe.clone() } },
                        }
                    }
                },
                View::Extras => rsx! { extras::Extras { recipe: recipe.clone(), figures: figures.clone() } },
                View::Water => rsx! { water::Water { recipe: recipe.clone() } },
                View::Brewday => rsx! { brewday::Brewday { recipe: recipe.clone(), figures: figures.clone() } },
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

/// The figures the Qt recipe tab shows: `SMART_FIELD_INIT_NO_SF` in `src/MainWindow.cpp`
/// plus the two labelled directly on it.
///
/// OG, FG and colour are shown and never typed; ABV, bitterness and IBU/GU follow from them.
/// IBU per gravity unit is how hoppy a beer is *for its strength*, which is why the dialog
/// shows it beside the two figures it is made of rather than leaving it to be worked out.
#[component]
fn Derived(figures: Calculated) -> Element {
    let gravity_units = (figures.og - 1.0) * 1000.0;
    let ibu_per_gu =
        if gravity_units > 0.0 { format!("{:.2}", figures.ibus / gravity_units) } else { NOTHING.to_owned() };

    rsx! {
        dl { class: "bt-derived",
            Figure { label: "OG", value: format!("{:.3}", figures.og) }
            Figure { label: "FG", value: format!("{:.3}", figures.fg) }
            Figure { label: "ABV", value: format!("{:.1}%", figures.abv_pct) }
            Figure { label: "Bitterness (IBU)", value: format!("{:.0}", figures.ibus) }
            Figure { label: "IBU/GU", value: ibu_per_gu }
            Figure { label: "Color", value: format!("{:.1} SRM", figures.color_srm) }
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
