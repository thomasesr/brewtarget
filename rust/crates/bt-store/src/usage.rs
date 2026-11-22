// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/usage.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from Recipe::numRecipesUsing in src/model/Recipe.cpp

//! How many recipes use each thing.
//!
//! The "N° Recipes" column every catalogue ends with.  Ported from `Recipe::numRecipesUsing`:
//! *distinct* recipes, neither the recipe nor the addition deleted.  Distinct matters because a
//! recipe may well use the same hop three times, and that is still one recipe.
//!
//! Counted for a whole table at once rather than a row at a time: a catalogue of 826 hops would
//! otherwise be 826 queries.

use std::collections::BTreeMap;

use crate::{Store, StoreError, dispatch};

/// How something is reached from a recipe.
enum Through {
    /// An addition table, which names both the recipe and the ingredient.
    Additions { table: &'static str, column: &'static str },
    /// A column on the recipe itself.
    RecipeColumn(&'static str),
}

/// How each catalogue's rows are referred to by a recipe.
fn through(table: &str) -> Option<Through> {
    Some(match table {
        "fermentable" => Through::Additions { table: "fermentable_in_recipe", column: "fermentable_id" },
        "hop" => Through::Additions { table: "hop_in_recipe", column: "hop_id" },
        "misc" => Through::Additions { table: "misc_in_recipe", column: "misc_id" },
        "yeast" => Through::Additions { table: "yeast_in_recipe", column: "yeast_id" },
        // These are named by the recipe rather than added to it.
        "style" => Through::RecipeColumn("style_id"),
        "equipment" => Through::RecipeColumn("equipment_id"),
        "mash" => Through::RecipeColumn("mash_id"),
        "boil" => Through::RecipeColumn("boil_id"),
        "fermentation" => Through::RecipeColumn("fermentation_id"),
        // Water is referred to twice over, as the base and the target, so it is counted
        // separately below.
        "water" => return None,
        _ => return None,
    })
}

/// How many recipes use each row of a table, keyed by that row's id.
///
/// Rows nothing uses are absent rather than zero; the caller treats a missing count as none.
pub async fn recipe_counts(store: &Store, table: &str) -> Result<BTreeMap<i32, i64>, StoreError> {
    let sql = match through(table) {
        Some(Through::Additions { table: additions, column }) => format!(
            "SELECT a.{column} AS id, COUNT(DISTINCT a.recipe_id) AS used \
             FROM {additions} a JOIN recipe r ON r.id = a.recipe_id \
             WHERE a.{column} IS NOT NULL \
               AND (a.deleted IS NULL OR a.deleted = FALSE) \
               AND (r.deleted IS NULL OR r.deleted = FALSE) \
             GROUP BY a.{column}"
        ),
        Some(Through::RecipeColumn(column)) => format!(
            "SELECT r.{column} AS id, COUNT(*) AS used FROM recipe r \
             WHERE r.{column} IS NOT NULL AND (r.deleted IS NULL OR r.deleted = FALSE) \
             GROUP BY r.{column}"
        ),
        None if table == "water" => {
            // A recipe names water twice -- the base it starts from and the target it aims at --
            // and a recipe that uses one profile for both still uses it once.
            "SELECT id, COUNT(DISTINCT recipe_id) AS used FROM ( \
               SELECT water_base AS id, id AS recipe_id FROM recipe \
                 WHERE water_base IS NOT NULL AND (deleted IS NULL OR deleted = FALSE) \
               UNION ALL \
               SELECT water_target AS id, id AS recipe_id FROM recipe \
                 WHERE water_target IS NOT NULL AND (deleted IS NULL OR deleted = FALSE) \
             ) AS both GROUP BY id"
                .to_owned()
        }
        None => return Ok(BTreeMap::new()),
    };

    let found = dispatch!(store, |pool| {
        use sqlx::Row;
        sqlx::query(&sql)
            .fetch_all(pool)
            .await?
            .iter()
            .filter_map(|row| {
                let id: i32 = row.try_get("id").ok()?;
                let used: i64 = row.try_get("used").ok()?;
                Some((id, used))
            })
            .collect::<BTreeMap<i32, i64>>()
    });
    Ok(found)
}
