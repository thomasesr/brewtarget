// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/preferences.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/optionsDialog.ui and src/OptionDialog.cpp

//! What the brewer has chosen: units, formulae, and how much to credit odd hop additions.
//!
//! The Qt application keeps these in `QSettings`, which is a file beside the database rather
//! than in it.  This keeps them the same way -- a file in the data directory -- because the
//! schema is frozen and a preference is not brewing data.  Stage 2 moves them to the account,
//! which is a change of where the file is read from and nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use bt_calc::color::ColorFormula;
use bt_calc::ibu::Formula as IbuFormula;
use bt_calc::recipe_ibu::IbuSettings;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(read).put(write))
}

/// Everything the options dialog sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default)]
pub struct Preferences {
    /// Which system of measurement to show each physical quantity in, by the quantity's name:
    /// `Mass` to `UsCustomary`, say.  A quantity not named here is shown in its default.
    pub units: BTreeMap<String, String>,
    /// How many decimal places to show each physical quantity to.
    pub precision: BTreeMap<String, u8>,
    pub ibu_formula: String,
    pub color_formula: String,
    /// What share of a mash hop's bitterness to credit, as a percentage.
    pub mash_hop_pct: f64,
    /// ...and of a first wort hop's.
    pub first_wort_hop_pct: f64,
}

impl Default for Preferences {
    fn default() -> Self {
        let ibu = IbuSettings::default();
        Self {
            units: BTreeMap::new(),
            precision: BTreeMap::new(),
            ibu_formula: ibu.formula.as_str().to_owned(),
            color_formula: ColorFormula::default().as_str().to_owned(),
            // Stored as percentages because that is what the dialog shows.
            mash_hop_pct: ibu.mash_hop_adjustment * 100.0,
            first_wort_hop_pct: ibu.first_wort_hop_adjustment * 100.0,
        }
    }
}

impl Preferences {
    /// The IBU settings these amount to.
    pub fn ibu(&self) -> IbuSettings {
        IbuSettings {
            formula: self.ibu_formula.parse().unwrap_or_default(),
            mash_hop_adjustment: self.mash_hop_pct / 100.0,
            first_wort_hop_adjustment: self.first_wort_hop_pct / 100.0,
        }
    }

    /// The colour formula these amount to.
    pub fn color(&self) -> ColorFormula {
        self.color_formula.parse().unwrap_or_default()
    }

    /// Where the preferences are kept.
    pub fn path(data_directory: &Path) -> PathBuf {
        data_directory.join("preferences.json")
    }

    /// Read them, falling back to the defaults.
    ///
    /// A file that cannot be read or does not parse gives the defaults rather than an error: a
    /// broken preferences file should not stop the application opening.
    pub fn load(data_directory: &Path) -> Self {
        std::fs::read_to_string(Self::path(data_directory))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write them.
    pub fn save(&self, data_directory: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(Self::path(data_directory), text)
    }

    /// Check that every choice names something real.
    ///
    /// A system of measurement that does not exist, or one that exists but not for that
    /// quantity, would silently fall back at display time; saying so is better.
    pub fn check(&self) -> Result<(), String> {
        for (quantity, system) in &self.units {
            let found = bt_measure::UnitSystem::all().any(|candidate| {
                candidate.physical_quantity() == quantity && candidate.system_of_measurement() == system
            });
            if !found {
                return Err(format!("{quantity} is not measured in {system}"));
            }
        }
        if self.ibu_formula.parse::<IbuFormula>().is_err() {
            return Err(format!("{:?} is not a bitterness formula", self.ibu_formula));
        }
        if self.color_formula.parse::<ColorFormula>().is_err() {
            return Err(format!("{:?} is not a colour formula", self.color_formula));
        }
        Ok(())
    }
}

#[utoipa::path(get, path = "/api/preferences", tag = "preferences",
    responses((status = 200, description = "What the brewer has chosen", body = Preferences)))]
async fn read(State(state): State<AppState>) -> Json<Preferences> {
    Json(Preferences::load(state.state_directory()))
}

#[utoipa::path(put, path = "/api/preferences", tag = "preferences", request_body = Preferences,
    responses(
        (status = 200, description = "What was stored", body = Preferences),
        (status = 400, description = "A choice naming something that does not exist", body = crate::error::ErrorBody),
    ))]
async fn write(State(state): State<AppState>, Json(preferences): Json<Preferences>) -> ApiResult<Json<Preferences>> {
    preferences.check().map_err(ApiError::BadRequest)?;
    preferences
        .save(state.state_directory())
        .map_err(|error| ApiError::Internal(format!("could not save the preferences: {error}")))?;
    Ok(Json(preferences))
}

/// The preference routes, for the API description.
#[derive(utoipa::OpenApi)]
#[openapi(paths(read, write), components(schemas(Preferences)))]
pub struct PreferencesApi;
