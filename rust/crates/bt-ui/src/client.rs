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
use serde_json::Value;

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

/// A request that never got anywhere, said with the address it was trying.
///
/// `reqwest`'s own message for a URL it cannot parse is "builder error", which says nothing
/// about what was wrong with it.
pub(crate) fn unreachable(url: &str, error: reqwest::Error) -> ApiError {
    ApiError::Unreachable(format!("{error} ({url})"))
}

/// The shape `bt_api::error::ErrorBody` arrives in.
#[derive(Debug, serde::Deserialize)]
struct Refusal {
    kind: String,
    message: String,
}

/// Where the API is when nothing has said otherwise.
///
/// In a browser that is wherever the page came from, because `bt-server` serves both the page
/// and the API.  It has to be asked for and written out in full: `reqwest` builds on `Url`,
/// which cannot parse `/api/hop` without a base, and the failure is an opaque "builder error"
/// rather than anything that points at the cause.
fn default_base() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| window.location().origin().ok())
            // A page with no origin is a `file://` one or a sandboxed frame.  There is nothing
            // useful to guess, so this leaves the base empty and lets the first request say so.
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Nothing to ask.  `bt-server`'s own default port, so a desktop build pointed at a
        // local server works without being told; anything else passes its own base.
        "http://127.0.0.1:8080".to_owned()
    }
}

/// What an import did.  Mirrors `bt_api::transfer::ImportSummary`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Imported {
    /// Which of the three formats the file turned out to be.
    pub format: String,
    pub rows_written: usize,
    pub recipe_ids: Vec<i32>,
}

/// A connection to a Brewtarget server.
#[derive(Debug, Clone)]
pub struct Client {
    /// Where the API is, with no trailing slash.  Always absolute: see [`default_base`].
    base: String,
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(default_base())
    }
}

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into().trim_end_matches('/').to_owned(), http: reqwest::Client::new() }
    }

    /// Where the API is.
    pub fn base(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api{path}", self.base)
    }

    /// Say plainly that the base is unusable, rather than letting `reqwest` fail to build a
    /// request and report only that it failed to build a request.
    fn check_base(&self) -> Result<(), ApiError> {
        if self.base.starts_with("http://") || self.base.starts_with("https://") {
            return Ok(());
        }
        Err(ApiError::Unreachable(format!(
            "{:?} is not a usable address for the server; it needs a scheme and a host",
            self.base
        )))
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
        self.check_base()?;
        let url = self.url(path);
        let response = self.http.get(&url).send().await.map_err(|error| unreachable(&url, error))?;
        Self::read(response).await
    }

    async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        self.check_base()?;
        let url = self.url(path);
        let response =
            self.http.request(method, &url).json(body).send().await.map_err(|error| unreachable(&url, error))?;
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
                    self.check_base()?;
                    let url = self.url(&format!("/{}/{id}", $table));
                    let response =
                        self.http.delete(&url).send().await.map_err(|error| $crate::client::unreachable(&url, error))?;
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
    /// Read one row of any table, as the columns it holds rather than as a typed struct.
    ///
    /// The editor works this way because it is built from the schema: it is handed a list of
    /// columns and has to show a control for each, which a struct known only at compile time
    /// cannot give it.
    pub async fn row(&self, table: &str, id: i32) -> Result<serde_json::Map<String, Value>, ApiError> {
        self.get(&format!("/{table}/{id}")).await
    }

    /// Store a row of any table, replacing what was there.
    pub async fn put_row(
        &self,
        table: &str,
        id: i32,
        row: &serde_json::Map<String, Value>,
    ) -> Result<serde_json::Map<String, Value>, ApiError> {
        self.send(reqwest::Method::PUT, &format!("/{table}/{id}"), row).await
    }

    /// Store a new row of any table, and read back what was stored -- including the key the
    /// database gave it.
    pub async fn post_row(
        &self,
        table: &str,
        row: &serde_json::Map<String, Value>,
    ) -> Result<serde_json::Map<String, Value>, ApiError> {
        self.send(reqwest::Method::POST, &format!("/{table}"), row).await
    }

    /// Delete a row of any table.
    ///
    /// Soft where the table records it: the row stays so that anything referring to it still
    /// resolves, and stops being listed.  That is the server's decision, not this one's.
    pub async fn delete_row(&self, table: &str, id: i32) -> Result<(), ApiError> {
        self.check_base()?;
        let url = self.url(&format!("/{table}/{id}"));
        let response = self.http.delete(&url).send().await.map_err(|error| unreachable(&url, error))?;
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

    /// Every row of a table, as columns rather than as a typed struct.
    ///
    /// What a picker needs: a list of rows of the table a foreign key names, to choose one of.
    pub async fn rows(&self, table: &str) -> Result<Vec<serde_json::Map<String, Value>>, ApiError> {
        self.get(&format!("/{table}")).await
    }

    /// Read a file and store everything in it.
    ///
    /// The whole file goes in the body: that is what a browser already has after reading one,
    /// and wrapping it in a form buys nothing.  The server works out which of the three formats
    /// it is from the contents rather than the name.
    pub async fn import(&self, document: &str) -> Result<Imported, ApiError> {
        self.check_base()?;
        let url = self.url("/import");
        let response = self
            .http
            .post(&url)
            .header("content-type", "text/plain")
            .body(document.to_owned())
            .send()
            .await
            .map_err(|error| unreachable(&url, error))?;
        Self::read(response).await
    }

    /// Where the whole database can be downloaded from, in one of the three formats.
    ///
    /// An address rather than a request: the browser does the downloading, so it gets a real
    /// file with a name rather than something held in memory.
    pub fn export_url(&self, format: &str) -> String {
        format!("{}?format={format}", self.url("/export"))
    }

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
