// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/recipes.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! What a recipe works out to.
//!
//! The generated routes give a recipe's row; these give the recipe itself -- the ingredients it
//! uses, the figures the recipe screen shows, and the instruction sheet for brewing it.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use bt_calc::recipe::Calculated;
use bt_model::recipe::Recipe;
use serde::Serialize;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{id}/full", get(full))
        .route("/{id}/calculated", get(calculated))
        .route("/{id}/brewday", get(brewday))
}

async fn load(state: &AppState, id: i32) -> ApiResult<Recipe> {
    bt_store::recipes::load(state.store(), id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("no recipe with id {id}")))
}

#[utoipa::path(
    get, path = "/api/recipes/{id}/full", tag = "recipes",
    params(("id" = i32, Path, description = "The key of the recipe")),
    responses(
        (status = 200, description = "The recipe, its additions and its profiles", body = Recipe),
        (status = 404, description = "No recipe with that key", body = ErrorBody),
    ),
)]
/// The whole recipe: its row, its additions with the ingredients they name, and its profiles.
async fn full(State(state): State<AppState>, Path(id): Path<i32>) -> ApiResult<Json<Recipe>> {
    Ok(Json(load(&state, id).await?))
}

#[utoipa::path(
    get, path = "/api/recipes/{id}/calculated", tag = "recipes",
    params(("id" = i32, Path, description = "The key of the recipe")),
    responses(
        (status = 200, description = "What the recipe works out to", body = Calculated),
        (status = 404, description = "No recipe with that key", body = ErrorBody),
    ),
)]
/// Everything the recipe screen shows: gravity, colour, bitterness, alcohol, volumes.
async fn calculated(State(state): State<AppState>, Path(id): Path<i32>) -> ApiResult<Json<Calculated>> {
    let recipe = load(&state, id).await?;
    let preferences = state.preferences();
    Ok(Json(bt_calc::recipe::calculate_recipe(&recipe, preferences.color_formula, preferences.ibu)))
}

/// One thing to do on brew day.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BrewdayStep {
    pub number: i32,
    pub name: String,
    pub directions: String,
    /// Where in the brew this belongs, so the sheet can be grouped.
    pub stage: Option<String>,
}

/// The brew day sheet.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Brewday {
    pub recipe_id: i32,
    pub recipe_name: String,
    pub batch_size_l: Option<f64>,
    pub boil_time_mins: f64,
    pub calculated: Calculated,
    pub steps: Vec<BrewdayStep>,
}

#[utoipa::path(
    get, path = "/api/recipes/{id}/brewday", tag = "recipes",
    params(("id" = i32, Path, description = "The key of the recipe")),
    responses(
        (status = 200, description = "The sheet to brew from", body = Brewday),
        (status = 404, description = "No recipe with that key", body = ErrorBody),
    ),
)]
/// The instruction sheet for brewing a recipe.
async fn brewday(State(state): State<AppState>, Path(id): Path<i32>) -> ApiResult<Json<Brewday>> {
    let recipe = load(&state, id).await?;
    let preferences = state.preferences();

    // The instructions the recipe carries, where it carries any; otherwise the ones the
    // recipe implies.  `Recipe::generateInstructions` is how the Qt application fills a brew
    // day sheet, and a recipe imported from a file usually has none until it is asked for.
    let stored: Vec<BrewdayStep> = recipe
        .instructions
        .iter()
        .filter(|instruction| !instruction.deleted.unwrap_or(false))
        .map(|instruction| BrewdayStep {
            number: instruction.step_number.unwrap_or_default(),
            name: instruction.name.clone().unwrap_or_default(),
            directions: instruction.directions.clone().unwrap_or_default(),
            stage: None,
        })
        .collect();
    let steps = if stored.is_empty() {
        bt_calc::instructions::generate(&recipe)
            .into_iter()
            .map(|instruction| BrewdayStep {
                number: instruction.step_number,
                name: instruction.name,
                directions: instruction.directions,
                stage: None,
            })
            .collect()
    } else {
        stored
    };

    Ok(Json(Brewday {
        recipe_id: recipe.id(),
        recipe_name: recipe.name().to_owned(),
        batch_size_l: recipe.row.batch_size,
        boil_time_mins: bt_calc::recipe::boil_time_mins(&recipe),
        calculated: bt_calc::recipe::calculate_recipe(&recipe, preferences.color_formula, preferences.ibu),
        steps,
    }))
}

/// The recipe routes, for the API description.
#[derive(utoipa::OpenApi)]
#[openapi(paths(full, calculated, brewday), components(schemas(Brewday, BrewdayStep)))]
pub struct RecipeApi;
