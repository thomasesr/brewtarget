// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/client.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Talking to `bt-api`.
//!
//! One client for every platform.  In a browser `reqwest` is a thin wrapper over `fetch`, and on
//! a desktop it opens its own connections; the calls written here are the same either way.
//!
//! The types that come back are `bt_model` and `bt_calc`'s own, so nothing is restated: a column
//! added to a row struct appears here without anything being written twice.

use bt_calc::recipe::Calculated;
use bt_model::recipe::Recipe;
use bt_model::tables::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Something that went wrong between asking and being answered.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// The request never arrived, or the answer never came back.
    #[error("could not reach the server: {0}")]
    Unreachable(String),
    /// The server answered, and the answer was a refusal.
    #[error("{message}")]
    Refused { status: u16, kind: String, message: String },
    /// The answer arrived but was not what this version expects.
    #[error("the server's answer could not be read: {0}")]
    Unreadable(String),
}

impl ApiError {
    /// Whether this is the server saying the thing asked for is not there.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Refused { status: 404, .. })
    }
}

/// The shape `bt_api::error::ErrorBody` arrives in.
#[derive(Debug, serde::Deserialize)]
struct Refusal {
    kind: String,
    message: String,
}

/// A connection to a Brewtarget server.
#[derive(Debug, Clone)]
pub struct Client {
    /// Where the API is, with no trailing slash.  Empty means "wherever this page came from",
    /// which is the usual case in a browser: the server that served the page serves the API.
    base: String,
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new("")
    }
}

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_owned(), http: reqwest::Client::new() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api{path}", self.base)
    }

    /// Read the answer, turning a refusal into an error rather than a value.
    async fn read<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ApiError> {
        let status = response.status();
        let body = response.text().await.map_err(|error| ApiError::Unreadable(error.to_string()))?;
        if !status.is_success() {
            // The server describes what it refused; if it did not, the status is all there is.
            let refusal = serde_json::from_str::<Refusal>(&body)
                .unwrap_or(Refusal { kind: "unknown".to_owned(), message: format!("the server answered {status}") });
            return Err(ApiError::Refused { status: status.as_u16(), kind: refusal.kind, message: refusal.message });
        }
        serde_json::from_str(&body).map_err(|error| ApiError::Unreadable(error.to_string()))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let response =
            self.http.get(self.url(path)).send().await.map_err(|error| ApiError::Unreachable(error.to_string()))?;
        Self::read(response).await
    }

    async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let response = self
            .http
            .request(method, self.url(path))
            .json(body)
            .send()
            .await
            .map_err(|error| ApiError::Unreachable(error.to_string()))?;
        Self::read(response).await
    }
}

/// Give a table the four calls every table answers.
///
/// The server generates these routes from the schema; this generates the calls to them from the
/// same list, so the two cannot drift apart.
macro_rules! table_calls {
    ($($list:ident, $read:ident, $create:ident, $update:ident, $delete:ident => $row:ty, $table:literal;)+) => {
        impl Client {
            $(
                #[doc = concat!("Every row of `", $table, "`.")]
                pub async fn $list(&self) -> Result<Vec<$row>, ApiError> {
                    self.get(concat!("/", $table)).await
                }

                #[doc = concat!("One row of `", $table, "`, by its key.")]
                pub async fn $read(&self, id: i32) -> Result<$row, ApiError> {
                    self.get(&format!("/{}/{id}", $table)).await
                }

                #[doc = concat!("Store a new row of `", $table, "`, and read back what was stored.")]
                pub async fn $create(&self, row: &$row) -> Result<$row, ApiError> {
                    self.send(reqwest::Method::POST, concat!("/", $table), row).await
                }

                #[doc = concat!("Replace a row of `", $table, "`.")]
                pub async fn $update(&self, id: i32, row: &$row) -> Result<$row, ApiError> {
                    self.send(reqwest::Method::PUT, &format!("/{}/{id}", $table), row).await
                }

                #[doc = concat!("Delete a row of `", $table, "`.")]
                pub async fn $delete(&self, id: i32) -> Result<(), ApiError> {
                    let response = self
                        .http
                        .delete(self.url(&format!("/{}/{id}", $table)))
                        .send()
                        .await
                        .map_err(|error| ApiError::Unreachable(error.to_string()))?;
                    let status = response.status();
                    if status.is_success() {
                        return Ok(());
                    }
                    Err(ApiError::Refused {
                        status: status.as_u16(),
                        kind: "unknown".to_owned(),
                        message: format!("the server answered {status}"),
                    })
                }
            )+
        }
    };
}

table_calls! {
    fermentables, fermentable, create_fermentable, update_fermentable, delete_fermentable => FermentableRow, "fermentable";
    hops, hop, create_hop, update_hop, delete_hop => HopRow, "hop";
    miscs, misc, create_misc, update_misc, delete_misc => MiscRow, "misc";
    yeasts, yeast, create_yeast, update_yeast, delete_yeast => YeastRow, "yeast";
    waters, water, create_water, update_water, delete_water => WaterRow, "water";
    styles, style, create_style, update_style, delete_style => StyleRow, "style";
    equipments, equipment, create_equipment, update_equipment, delete_equipment => EquipmentRow, "equipment";
    mashes, mash, create_mash, update_mash, delete_mash => MashRow, "mash";
    boils, boil, create_boil, update_boil, delete_boil => BoilRow, "boil";
    fermentations, fermentation, create_fermentation, update_fermentation, delete_fermentation => FermentationRow, "fermentation";
    recipe_rows, recipe_row, create_recipe, update_recipe, delete_recipe => RecipeRow, "recipe";
}

impl Client {
    /// A recipe with its additions and the ingredients they name.
    pub async fn recipe(&self, id: i32) -> Result<Recipe, ApiError> {
        self.get(&format!("/recipes/{id}/full")).await
    }

    /// What a recipe works out to, as the server reckons it.
    ///
    /// The same arithmetic is in `bt_calc` and could be run here, and for a recipe being edited
    /// it should be -- that is what makes the figures move as you type.  This is for a recipe
    /// that has not been touched, where asking is cheaper than fetching everything it refers to.
    pub async fn calculated(&self, id: i32) -> Result<Calculated, ApiError> {
        self.get(&format!("/recipes/{id}/calculated")).await
    }
}
