// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/error.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! What a request says when it cannot be done.
//!
//! One shape for every failure, so a client never has to guess: a status, a short machine-
//! readable kind, and a message.  Nothing here reveals the database's own error text, which can
//! name tables and columns a caller has no business knowing about.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    BadRequest(String),
    #[error(transparent)]
    Store(#[from] bt_store::StoreError),
    #[error(transparent)]
    Format(#[from] bt_format::mapping::MappingError),
}

/// The body of a failed request.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// A short, stable word for what went wrong: `not_found`, `bad_request`, `internal`.
    pub kind: &'static str,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, message) = match &self {
            ApiError::NotFound(what) => (StatusCode::NOT_FOUND, "not_found", what.clone()),
            ApiError::BadRequest(why) => (StatusCode::BAD_REQUEST, "bad_request", why.clone()),
            ApiError::Format(error) => (StatusCode::BAD_REQUEST, "bad_request", error.to_string()),
            ApiError::Store(error) => {
                // A database failure is ours, not the caller's, so it is logged in full and
                // reported in outline.  The detail can name tables and columns.
                tracing::error!(%error, "a request failed in the storage layer");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal", "the request could not be completed".to_owned())
            }
        };
        (status, Json(ErrorBody { kind, message })).into_response()
    }
}

/// What a handler returns.
pub type ApiResult<T> = Result<T, ApiError>;
