// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/transfer.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Getting recipes in and out.
//!
//! The three interchange formats, over HTTP.  Which one a file is in is decided by what is in it
//! rather than by what the caller claims: a `.beer` file renamed to `.xml` is still a `.beer`
//! file, and guessing from the name would reject it for no reason.

use axum::extract::{DefaultBodyLimit, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use bt_format::document::{self, Document};
use bt_format::{jsonc, xml};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// How big a file may be to import.
///
/// A whole library arrives in one request: the content that ships with the application is a
/// little over 2 MB as `.beer`, and someone's own collection can be larger, so axum's 2 MB
/// default would turn an ordinary import into a refusal.
const LARGEST_IMPORT_BYTES: usize = 64 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/import", post(import).layer(DefaultBodyLimit::max(LARGEST_IMPORT_BYTES)))
        .route("/export", get(export))
}

/// Which interchange format a document is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// Every row the API returns is named after its columns, which are snake_case, so everything
// alongside them is too rather than making the caller switch conventions halfway through a
// response.
#[serde(rename_all = "snake_case")]
pub enum Format {
    DotBeer,
    BeerJson,
    BeerXml,
}

/// Work out what a file is from its contents.
///
/// XML announces itself with a declaration.  Between the two JSON formats the root tells them
/// apart: `.beer` files are rooted at `DotBeer` and BeerJSON files at `beerjson`.
pub fn detect(source: &str) -> Option<Format> {
    if source.trim_start().starts_with("<?xml") {
        return Some(Format::BeerXml);
    }
    let parsed = jsonc::value_from_str(source).ok()?;
    if parsed.get("DotBeer").is_some() {
        return Some(Format::DotBeer);
    }
    if parsed.get("beerjson").is_some() {
        return Some(Format::BeerJson);
    }
    None
}

/// Read a document, whichever of the three it is.
pub fn read(source: &str) -> ApiResult<(Format, Document)> {
    let format = detect(source)
        .ok_or_else(|| ApiError::BadRequest("this is not a .beer, BeerJSON or BeerXML document".to_owned()))?;
    let document = match format {
        Format::BeerXml => {
            let parsed = xml::read(source).map_err(|error| ApiError::BadRequest(error.to_string()))?;
            document::read_beerxml(&parsed)?
        }
        Format::DotBeer | Format::BeerJson => {
            let parsed = jsonc::value_from_str(source).map_err(|error| ApiError::BadRequest(error.to_string()))?;
            if format == Format::DotBeer { document::read_dotbeer(&parsed)? } else { document::read_beerjson(&parsed)? }
        }
    };
    Ok((format, document))
}

/// What an import did.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportSummary {
    pub format: Format,
    pub rows_written: usize,
    pub recipe_ids: Vec<i32>,
}

/// Read a file and store everything in it.
///
/// The body is the file itself rather than a form, because that is what a client already has and
/// wrapping it buys nothing.
async fn import(State(state): State<AppState>, body: String) -> ApiResult<Json<ImportSummary>> {
    let (format, document) = read(&body)?;
    let imported = bt_store::import::import(state.store(), &document).await?;
    Ok(Json(ImportSummary { format, rows_written: imported.rows_written, recipe_ids: imported.recipe_ids }))
}

/// What to export.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExportQuery {
    /// Which format to write; `.beer` by default, as the most expressive of the three.
    #[serde(default)]
    pub format: Option<Format>,
}

/// Write out everything in the database.
///
/// Stage 2 exports one account's store, which is this same call against a different store.
async fn export(
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;

    let document = bt_store::export::export(state.store()).await?;
    let format = query.format.unwrap_or(Format::DotBeer);

    let (body, content_type) = match format {
        Format::BeerXml => (xml::write(&document::write_beerxml(&document)?), "application/xml"),
        Format::BeerJson => (
            serde_json::to_string_pretty(&document::write_beerjson(&document)?).unwrap_or_default(),
            "application/json",
        ),
        Format::DotBeer => {
            (serde_json::to_string_pretty(&document::write_dotbeer(&document)?).unwrap_or_default(), "application/json")
        }
    };
    Ok(([(axum::http::header::CONTENT_TYPE, content_type)], body).into_response())
}
