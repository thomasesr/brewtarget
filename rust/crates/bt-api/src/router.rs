// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/router.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Everything the API serves.
//!
//! Routes are grouped rather than listed flat so that a session layer can be wrapped around the
//! data routes in Stage 2 without touching a handler -- which is forward-compatibility
//! constraint 4 in PLAN.md §3.3.  `/healthz` stays outside that group: a load balancer has to be
//! able to ask whether the process is alive without being logged in, and it deliberately reports
//! nothing but that.

use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::state::AppState;

/// What `/healthz` says.
///
/// Liveness only: no version, no schema, nothing about what is inside.  Anything more is a
/// detail an unauthenticated caller has no business knowing.
#[derive(Debug, Serialize)]
pub struct Health {
    pub status: &'static str,
}

async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

/// The routes that read and write brewing data.
///
/// In Stage 2 this whole group goes behind a session layer in one place.
pub fn data_routes() -> Router<AppState> {
    Router::new()
        .merge(crate::generated::routes::table_routes())
        .nest("/recipes", crate::recipes::routes())
        .nest("/inventory", crate::inventory::routes())
        .nest("/usage", crate::inventory::usage_routes())
        // The description of the API is part of the API, so it sits inside the group a session
        // layer will eventually wrap: what you may call is what you may read about.
        .route("/openapi.json", get(describe))
        .merge(crate::transfer::routes())
}

/// The whole API.
pub fn api() -> Router<AppState> {
    Router::new().route("/healthz", get(healthz)).nest("/api", data_routes())
}

/// The whole API, ready to serve.
pub fn router(state: AppState) -> Router {
    api().with_state(state)
}

/// The API, described: the document a typed client is generated from.
async fn describe() -> Json<utoipa::openapi::OpenApi> {
    Json(crate::openapi::document())
}
