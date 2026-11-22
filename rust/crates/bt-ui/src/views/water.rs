// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/water.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/recipeWaterWidget.ui and src/widgets/RecipeWaterWidget.cpp

//! The Water tab: which water the recipe starts from and which it is aiming at.
//!
//! The Qt widget also shows what the water in the kettle actually comes to -- the "Actual"
//! group of ion figures, and the water agent and acid malt tables that adjust them.  Those
//! need the water chemistry from `src/model/Water.cpp` and the auto-adjuster, which this port
//! has not reached (PLAN.md Phase 5).  Showing a figure worked out some other way would be
//! worse than showing none, so what is not ported is said rather than guessed.

use bt_model::recipe::Recipe;
use bt_model::tables::WaterRow;
use dioxus::prelude::*;

use super::format::{NOTHING, number, percent, text};

#[component]
pub fn Water(recipe: Recipe) -> Element {
    let row = &recipe.row;

    rsx! {
        dl { class: "bt-facts",
            Entry {
                label: "Base Profile",
                value: text(recipe.water_base.as_ref().and_then(|water| water.name.as_deref())),
            }
            Entry {
                label: "Target Profile",
                value: text(recipe.water_target.as_ref().and_then(|water| water.name.as_deref())),
            }
            Entry { label: "% RO in Mash", value: percent(row.ro_water_mash_pct) }
            Entry { label: "% RO in Sparge", value: percent(row.ro_water_sparge_pct) }
            Entry { label: "Adjust acid for mash pH of", value: number(row.target_mash_ph, 2) }
            Entry {
                label: "Keep water agents in sync",
                value: match row.water_kept_in_sync {
                    Some(true) => "Yes".to_owned(),
                    Some(false) => "No".to_owned(),
                    None => NOTHING.to_owned(),
                },
            }
        }

        if let Some(base) = recipe.water_base.as_ref() {
            h3 { class: "bt-group", "Base Profile" }
            Ions { water: base.clone() }
        }
        if let Some(target) = recipe.water_target.as_ref() {
            h3 { class: "bt-group", "Target Profile" }
            Ions { water: target.clone() }
        }

        p { class: "bt-note",
            "What the water in the kettle actually comes to, and the water agents and acid malts
             that get it there, are not ported yet: they need the water chemistry from
             src/model/Water.cpp."
        }
    }
}

/// The ions of one water profile, in the order the Qt widget lists them.
#[component]
fn Ions(water: WaterRow) -> Element {
    rsx! {
        dl { class: "bt-facts bt-ions",
            Entry { label: "Ca", value: number(water.calcium, 2) }
            Entry { label: "Mg", value: number(water.magnesium, 2) }
            Entry { label: "Na", value: number(water.sodium, 2) }
            Entry { label: "Cl", value: number(water.chloride, 2) }
            Entry { label: "SO₄", value: number(water.sulfate, 2) }
            Entry { label: "HCO₃", value: number(water.bicarbonate, 2) }
            Entry { label: "pH", value: number(water.ph, 2) }
        }
    }
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
