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
use bt_calc::mash;
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

    // The mash changes how fermentable the wort is, which changes the final gravity, so the
    // schedule has to be modelled before the recipe can be.
    let multiplier = if recipe.mash_steps().is_empty() {
        None
    } else {
        let steps: Vec<mash::Step> = recipe
            .mash_steps()
            .iter()
            .map(|step| mash::Step {
                kind: step_kind(step),
                step_temp_c: step.step_temp_c.unwrap_or(0.0),
                end_temp_c: step.end_temp_c,
                step_time_mins: step.step_time_mins.unwrap_or(0.0),
                ramp_time_mins: step.ramp_time_mins,
            })
            .collect();
        mash::attenuation_multiplier(&mash::build_profile(&steps), mash::enzyme_levels(None, None))
    };

    Ok(Json(bt_calc::recipe::calculate_with(&recipe, preferences.color_formula, multiplier, preferences.ibu)))
}

/// What kind of mash step this is, as the fermentability model sees it.
///
/// Ported from `Recipe::impl::fermentabilityStepType`: a sparge ends the mash as far as the
/// model is concerned, and a souring step is something else entirely.
fn step_kind(step: &bt_model::tables::MashStepRow) -> mash::StepType {
    use bt_model::enums::MashStepType;
    match step.mstype().ok().flatten() {
        Some(MashStepType::Infusion) => mash::StepType::Infusion,
        Some(MashStepType::Temperature) => mash::StepType::Temperature,
        Some(MashStepType::Decoction) => mash::StepType::Decoction,
        Some(MashStepType::FlySparge | MashStepType::BatchSparge) => mash::StepType::Sparge,
        _ => mash::StepType::Other,
    }
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

    let steps = recipe
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

    Ok(Json(Brewday {
        recipe_id: recipe.id(),
        recipe_name: recipe.name().to_owned(),
        batch_size_l: recipe.row.batch_size,
        boil_time_mins: bt_calc::recipe::boil_time_mins(&recipe),
        calculated: bt_calc::recipe::calculate_with(&recipe, preferences.color_formula, None, preferences.ibu),
        steps,
    }))
}

/// The recipe routes, for the API description.
#[derive(utoipa::OpenApi)]
#[openapi(paths(full, calculated, brewday), components(schemas(Brewday, BrewdayStep)))]
pub struct RecipeApi;
