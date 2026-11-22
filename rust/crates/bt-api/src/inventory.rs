// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/inventory.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! What is in the store cupboard.
//!
//! The lots themselves are ordinary rows, so buying and using are the generated
//! `*_stock_purchase` and `*_stock_use` routes.  These are the totals that come of them.

use std::str::FromStr;

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use bt_store::inventory::{Holding, Kind, Lot};
use serde::Deserialize;

use crate::error::{ApiError, ApiResult, ErrorBody};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(everything)).route("/{kind}", get(of_kind)).route("/{kind}/{id}", get(one))
}

/// How many recipes use each row of a table.
pub fn usage_routes() -> Router<AppState> {
    Router::new().route("/{table}", get(usage))
}

#[utoipa::path(
    get, path = "/api/usage/{table}", tag = "usage",
    params(("table" = String, Path, description = "The table to count usage of")),
    responses((status = 200, description = "How many recipes use each row, by its key")),
)]
/// How many recipes use each row of a table, keyed by that row's id.
///
/// The "N° Recipes" column every catalogue ends with.  Counted for the whole table at once,
/// because a catalogue of 826 hops asked one at a time would be 826 requests.
async fn usage(
    State(state): State<AppState>,
    Path(table): Path<String>,
) -> ApiResult<Json<std::collections::BTreeMap<String, i64>>> {
    if bt_model::tables::columns_of(&table).is_none() {
        return Err(ApiError::BadRequest(format!("{table:?} is not a table in this schema")));
    }
    let counts = bt_store::usage::recipe_counts(state.store(), &table).await?;
    // JSON object keys are strings, so the ids are rendered as such.
    Ok(Json(counts.into_iter().map(|(id, used)| (id.to_string(), used)).collect()))
}

/// Which holdings to list.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct Filter {
    /// Leave out ingredients that have run out.  `IngredientBase::isOnHand`.
    #[serde(default)]
    pub on_hand: bool,
}

fn filtered(holdings: Vec<Holding>, filter: &Filter) -> Vec<Holding> {
    if filter.on_hand { holdings.into_iter().filter(|holding| holding.on_hand).collect() } else { holdings }
}

/// Read the kind out of the path, saying which kinds there are when it is not one of them.
fn kind_of(kind: &str) -> ApiResult<Kind> {
    Kind::from_str(kind).map_err(|_| {
        let known = Kind::ALL.map(Kind::as_str).join(", ");
        ApiError::BadRequest(format!("{kind:?} is not something that can be stocked; expected one of {known}"))
    })
}

#[utoipa::path(
    get, path = "/api/inventory", tag = "inventory",
    params(Filter),
    responses((status = 200, description = "What is on hand of every ingredient", body = Vec<Holding>)),
)]
/// Everything on hand, of every kind.
async fn everything(State(state): State<AppState>, Query(filter): Query<Filter>) -> ApiResult<Json<Vec<Holding>>> {
    Ok(Json(filtered(bt_store::inventory::everything(state.store()).await?, &filter)))
}

#[utoipa::path(
    get, path = "/api/inventory/{kind}", tag = "inventory",
    params(("kind" = String, Path, description = "fermentable, hop, misc or yeast"), Filter),
    responses(
        (status = 200, description = "What is on hand of that kind", body = Vec<Holding>),
        (status = 400, description = "Not something that can be stocked", body = ErrorBody),
    ),
)]
/// Everything on hand of one kind.
async fn of_kind(
    State(state): State<AppState>,
    Path(kind): Path<String>,
    Query(filter): Query<Filter>,
) -> ApiResult<Json<Vec<Holding>>> {
    let holdings = bt_store::inventory::holdings(state.store(), kind_of(&kind)?).await?;
    Ok(Json(filtered(holdings, &filter)))
}

#[utoipa::path(
    get, path = "/api/inventory/{kind}/{id}", tag = "inventory",
    params(
        ("kind" = String, Path, description = "fermentable, hop, misc or yeast"),
        ("id" = i32, Path, description = "The key of the ingredient"),
    ),
    responses(
        (status = 200, description = "What is on hand, and the lots it is in", body = Holding),
        (status = 404, description = "Nothing has ever been bought of it", body = ErrorBody),
    ),
)]
/// What is on hand of one ingredient, including the lots it is held in.
async fn one(State(state): State<AppState>, Path((kind, id)): Path<(String, i32)>) -> ApiResult<Json<Holding>> {
    let kind = kind_of(&kind)?;
    bt_store::inventory::holdings(state.store(), kind)
        .await?
        .into_iter()
        .find(|holding| holding.ingredient_id == id)
        // Nothing has ever been bought, which is not the same as the ingredient not existing --
        // but there is no holding to describe either way.
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("nothing has been bought of {} {id}", kind.as_str())))
}

/// The inventory routes, for the API description.
#[derive(utoipa::OpenApi)]
#[openapi(paths(everything, of_kind, one, usage), components(schemas(Holding, Lot, Kind)))]
pub struct InventoryApi;
