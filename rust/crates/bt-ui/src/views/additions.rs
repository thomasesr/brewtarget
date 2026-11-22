// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/additions.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Columns ported from src/qtModels/tableModels/RecipeAddition*TableModel.cpp

//! What goes into a recipe.
//!
//! One tab per kind of addition, with the columns the matching Qt table shows.  An addition is
//! two things at once -- how much of what, and when -- so every one of these mixes columns read
//! from the ingredient with columns read from the addition itself.

use bt_calc::recipe::Calculated;
use bt_model::recipe::{Addition, IngredientAmount, Recipe};
use dioxus::prelude::*;

use super::format::{NOTHING, color_srm, label, number, percent, text};
use super::table::{Align, Cells, Column, DataTable};

/// How much of an ingredient an addition calls for, in the unit it was recorded in.
///
/// Amounts are stored as given rather than converted, because the unit chosen is part of what the
/// recipe says: 15 g of hops reads differently from 0.015 kg.
fn amount<Row: IngredientAmount, Ingredient>(addition: &Addition<Row, Ingredient>) -> String {
    match addition.addition.amount() {
        Some(amount) => format!("{} {}", number(Some(amount.quantity), 3), amount.unit.name()),
        None => NOTHING.to_owned(),
    }
}

/// When in the brew an addition is made.
fn stage<Row: bt_model::recipe::HasStage, Ingredient>(addition: &Addition<Row, Ingredient>) -> String {
    label(addition.addition.stage_value(), |value| value.display_name())
}

/// Which step of that stage, where the addition names one.
fn step(value: Option<i32>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => NOTHING.to_owned(),
    }
}

/// How long before the end of its stage the addition goes in.
fn at_time(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.0} min"),
        None => NOTHING.to_owned(),
    }
}

#[component]
pub fn Fermentables(recipe: Recipe) -> Element {
    let columns = vec![
        Column::of("fermentable", "name", Align::Start),
        Column::of("fermentable", "ftype", Align::Start),
        Column::of("fermentable", "fine_grind_yield_pct", Align::End),
        Column::of("fermentable", "color_lovibond", Align::End),
        Column::of("fermentable_in_recipe", "quantity", Align::End),
        Column::of("fermentable_in_recipe", "stage", Align::Start),
        Column::of("fermentable_in_recipe", "step", Align::End),
        Column::of("fermentable_in_recipe", "add_at_time_mins", Align::End),
    ];
    let rows: Vec<Cells> = recipe
        .fermentables
        .iter()
        .map(|addition| {
            let malt = addition.ingredient.as_ref();
            vec![
                text(malt.and_then(|malt| malt.name.as_deref())),
                malt.map(|malt| label(malt.ftype(), |value| value.display_name()))
                    .unwrap_or_else(|| NOTHING.to_owned()),
                percent(malt.and_then(|malt| malt.fine_grind_yield_pct)),
                color_srm(malt.and_then(|malt| malt.color_lovibond)),
                amount(addition),
                stage(addition),
                step(addition.addition.step),
                at_time(addition.addition.add_at_time_mins),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}

#[component]
pub fn Hops(recipe: Recipe, figures: Calculated) -> Element {
    let columns = vec![
        Column::of("hop", "name", Align::Start),
        Column::of("hop", "alpha_pct", Align::End),
        Column::of("hop_in_recipe", "quantity", Align::End),
        Column::of("hop_in_recipe", "stage", Align::Start),
        Column::of("hop_in_recipe", "step", Align::End),
        Column::of("hop_in_recipe", "add_at_time_mins", Align::End),
        Column::of("hop", "form", Align::Start),
        // Not a column anywhere: what this addition contributes, worked out here.
        Column::number("IBU"),
    ];
    let rows: Vec<Cells> = recipe
        .hops
        .iter()
        .enumerate()
        .map(|(index, addition)| {
            let hop = addition.ingredient.as_ref();
            vec![
                text(hop.and_then(|hop| hop.name.as_deref())),
                percent(hop.and_then(|hop| hop.alpha_pct)),
                amount(addition),
                stage(addition),
                step(addition.addition.step),
                at_time(addition.addition.add_at_time_mins),
                hop.map(|hop| label(hop.form(), |value| value.display_name())).unwrap_or_else(|| NOTHING.to_owned()),
                // What this addition contributes, which the Qt table does not show but the
                // recipe screen does -- and which is the figure a brewer is actually choosing
                // between when deciding what to change.
                number(figures.ibus_per_addition.get(index).copied(), 1),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}

#[component]
pub fn Miscs(recipe: Recipe) -> Element {
    let columns = vec![
        Column::of("misc", "name", Align::Start),
        Column::of("misc", "mtype", Align::Start),
        Column::of("misc_in_recipe", "quantity", Align::End),
        Column::of("misc_in_recipe", "stage", Align::Start),
        Column::of("misc_in_recipe", "step", Align::End),
        Column::of("misc_in_recipe", "add_at_time_mins", Align::End),
    ];
    let rows: Vec<Cells> = recipe
        .miscs
        .iter()
        .map(|addition| {
            let misc = addition.ingredient.as_ref();
            vec![
                text(misc.and_then(|misc| misc.name.as_deref())),
                misc.map(|misc| label(misc.mtype(), |value| value.display_name()))
                    .unwrap_or_else(|| NOTHING.to_owned()),
                amount(addition),
                stage(addition),
                step(addition.addition.step),
                at_time(addition.addition.add_at_time_mins),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}

#[component]
pub fn Yeasts(recipe: Recipe) -> Element {
    let columns = vec![
        Column::of("yeast", "name", Align::Start),
        Column::of("yeast", "laboratory", Align::Start),
        Column::of("yeast", "product_id", Align::Start),
        Column::of("yeast_in_recipe", "quantity", Align::End),
        Column::of("yeast_in_recipe", "stage", Align::Start),
        Column::of("yeast_in_recipe", "step", Align::End),
        Column::of("yeast", "ytype", Align::Start),
        Column::of("yeast", "form", Align::Start),
        Column::of("yeast_in_recipe", "attenuation_pct", Align::End),
    ];
    let rows: Vec<Cells> = recipe
        .yeasts
        .iter()
        .map(|addition| {
            let yeast = addition.ingredient.as_ref();
            vec![
                text(yeast.and_then(|yeast| yeast.name.as_deref())),
                text(yeast.and_then(|yeast| yeast.laboratory.as_deref())),
                text(yeast.and_then(|yeast| yeast.product_id.as_deref())),
                amount(addition),
                stage(addition),
                step(addition.addition.step),
                yeast
                    .map(|yeast| label(yeast.ytype(), |value| value.display_name()))
                    .unwrap_or_else(|| NOTHING.to_owned()),
                yeast
                    .map(|yeast| label(yeast.form(), |value| value.display_name()))
                    .unwrap_or_else(|| NOTHING.to_owned()),
                percent(addition.addition.attenuation_pct),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}
