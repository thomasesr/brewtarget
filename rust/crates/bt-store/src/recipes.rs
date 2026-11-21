// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/recipes.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Assembling a whole recipe out of the tables it is spread across.
//!
//! A `recipe` row says very little on its own: the additions live in four other tables, and the
//! mash, boil and fermentation are separate objects with their own steps.  This puts the graph
//! back together so the calculations have something to work from.

use bt_model::recipe::{Addition, Recipe};
use bt_model::tables::*;

use crate::{Store, StoreError, dispatch, tables};

/// Fetch rows of one table whose `column` matches `id`, in `order_by` order.
macro_rules! fetch_children {
    ($store:expr, $row:ty, $table:expr, $column:expr, $id:expr, $order_by:expr) => {{
        let sql =
            $store.placeholders(&format!("SELECT * FROM {} WHERE {} = ? ORDER BY {}", $table, $column, $order_by));
        dispatch!($store, |pool| { sqlx::query_as::<_, $row>(&sql).bind($id).fetch_all(pool).await? })
    }};
}

/// Pair each addition with the ingredient row it refers to.
///
/// An addition whose ingredient is missing is kept, with `None` for the ingredient: a foreign key
/// should make that impossible, but a hand-edited database is not obliged to be consistent, and
/// dropping the addition silently would change the recipe.
async fn with_ingredients<Row, Ingredient, F, Fut>(
    additions: Vec<Row>,
    ingredient_id: impl Fn(&Row) -> Option<i64>,
    fetch: F,
) -> Result<Vec<Addition<Row, Ingredient>>, StoreError>
where
    F: Fn(i64) -> Fut,
    Fut: std::future::Future<Output = Result<Option<Ingredient>, StoreError>>,
{
    let mut result = Vec::with_capacity(additions.len());
    for addition in additions {
        let ingredient = match ingredient_id(&addition) {
            Some(id) => fetch(id).await?,
            None => None,
        };
        result.push(Addition { addition, ingredient });
    }
    Ok(result)
}

/// Read a whole recipe, or `None` if there is no such row.
pub async fn load(store: &Store, id: i64) -> Result<Option<Recipe>, StoreError> {
    let Some(row) = tables::recipe::get(store, id).await? else {
        return Ok(None);
    };

    let fermentable_rows: Vec<FermentableInRecipeRow> =
        fetch_children!(store, FermentableInRecipeRow, "fermentable_in_recipe", "recipe_id", id, "id");
    let hop_rows: Vec<HopInRecipeRow> = fetch_children!(store, HopInRecipeRow, "hop_in_recipe", "recipe_id", id, "id");
    let misc_rows: Vec<MiscInRecipeRow> =
        fetch_children!(store, MiscInRecipeRow, "misc_in_recipe", "recipe_id", id, "id");
    let yeast_rows: Vec<YeastInRecipeRow> =
        fetch_children!(store, YeastInRecipeRow, "yeast_in_recipe", "recipe_id", id, "id");

    let fermentables =
        with_ingredients(fermentable_rows, |row| row.fermentable_id, |id| tables::fermentable::get(store, id)).await?;
    let hops = with_ingredients(hop_rows, |row| row.hop_id, |id| tables::hop::get(store, id)).await?;
    let miscs = with_ingredients(misc_rows, |row| row.misc_id, |id| tables::misc::get(store, id)).await?;
    let yeasts = with_ingredients(yeast_rows, |row| row.yeast_id, |id| tables::yeast::get(store, id)).await?;

    let equipment = match row.equipment_id {
        Some(id) => tables::equipment::get(store, id).await?,
        None => None,
    };
    let style = match row.style_id {
        Some(id) => tables::style::get(store, id).await?,
        None => None,
    };

    // Each of these is an object with its own ordered steps, so the row is fetched first and the
    // steps only if it is really there.
    let mash = match row.mash_id {
        Some(mash_id) => match tables::mash::get(store, mash_id).await? {
            Some(mash) => {
                Some((mash, fetch_children!(store, MashStepRow, "mash_step", "mash_id", mash_id, "step_number")))
            }
            None => None,
        },
        None => None,
    };
    let boil = match row.boil_id {
        Some(boil_id) => match tables::boil::get(store, boil_id).await? {
            Some(boil) => {
                Some((boil, fetch_children!(store, BoilStepRow, "boil_step", "boil_id", boil_id, "step_number")))
            }
            None => None,
        },
        None => None,
    };
    let fermentation = match row.fermentation_id {
        Some(fermentation_id) => match tables::fermentation::get(store, fermentation_id).await? {
            Some(fermentation) => Some((
                fermentation,
                fetch_children!(
                    store,
                    FermentationStepRow,
                    "fermentation_step",
                    "fermentation_id",
                    fermentation_id,
                    "step_number"
                ),
            )),
            None => None,
        },
        None => None,
    };

    let instructions: Vec<InstructionRow> =
        fetch_children!(store, InstructionRow, "instruction", "recipe_id", id, "step_number");

    Ok(Some(Recipe {
        row,
        fermentables,
        hops,
        miscs,
        yeasts,
        equipment,
        style,
        mash,
        boil,
        fermentation,
        instructions,
    }))
}
