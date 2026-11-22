// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/catalogues.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Columns ported from src/qtModels/tableModels/*TableModel.cpp

//! The ingredient catalogues.
//!
//! One table per kind of thing a recipe can refer to.  The columns are the ones the Qt tables
//! show, in the order they show them, taken from the `COLUMN_INFOS` block of each
//! `*TableModel.cpp` -- these are what a brewer expects to see, and reordering them would be a
//! redesign rather than a port.
//!
//! Not yet shown: the "N° Recipes" column every Qt table ends with, and the "Inventory" column on
//! the four ingredient tables.  Both need something the API does not offer yet -- a count of the
//! recipes referring to a row, and the inventory totals joined to it.

use bt_model::tables::*;
use dioxus::prelude::*;

use super::format::{color_srm, label, number, percent, text};
use super::table::{Cells, Column, DataTable, Failed, Loading};
use crate::Client;

/// Build a component that fetches one catalogue and shows it as a table.
///
/// The ten of them differ only in what they fetch and what they show, so they are written as
/// ten descriptions rather than ten components.
macro_rules! catalogue {
    ($component:ident, $fetch:ident, $row:ty, [$($header:expr => $cell:expr),+ $(,)?]) => {
        #[component]
        pub fn $component() -> Element {
            let client = use_context::<Client>();
            let rows = use_resource(move || {
                let client = client.clone();
                async move { client.$fetch().await }
            });

            let columns: Vec<Column> = vec![$($header),+];
            match &*rows.read_unchecked() {
                None => rsx! { Loading {} },
                Some(Err(error)) => rsx! { Failed { message: error.to_string() } },
                Some(Ok(found)) => {
                    let cells: Vec<Cells> = found
                        .iter()
                        // A row marked deleted is kept so that recipes referring to it still
                        // resolve, and is hidden from the catalogue -- as `BtTableModel` hides it.
                        .filter(|row: &&$row| !row.deleted.unwrap_or(false))
                        .map(|row| vec![$({ let cell: fn(&$row) -> String = $cell; cell(row) }),+])
                        .collect();
                    rsx! { DataTable { columns, rows: cells } }
                }
            }
        }
    };
}

catalogue!(Fermentables, fermentables, FermentableRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::text("Type") => |row| label(row.ftype(), |value| value.as_str()),
    Column::number("Yield %") => |row| percent(row.fine_grind_yield_pct),
    Column::number("Color") => |row| color_srm(row.color_lovibond),
]);

catalogue!(Hops, hops, HopRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::text("Form") => |row| label(row.form(), |value| value.as_str()),
    Column::text("Year") => |row| text(row.year.as_deref()),
    Column::number("Alpha %") => |row| percent(row.alpha_pct),
]);

catalogue!(Miscs, miscs, MiscRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::text("Type") => |row| label(row.mtype(), |value| value.as_str()),
]);

catalogue!(Yeasts, yeasts, YeastRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::text("Laboratory") => |row| text(row.laboratory.as_deref()),
    Column::text("Product ID") => |row| text(row.product_id.as_deref()),
    Column::text("Type") => |row| label(row.ytype(), |value| value.as_str()),
    Column::text("Form") => |row| label(row.form(), |value| value.as_str()),
]);

catalogue!(Waters, waters, WaterRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::number("Ca") => |row| number(row.calcium, 1),
    Column::number("HCO3") => |row| number(row.bicarbonate, 1),
    Column::number("SO4") => |row| number(row.sulfate, 1),
    Column::number("Cl") => |row| number(row.chloride, 1),
    Column::number("Na") => |row| number(row.sodium, 1),
    Column::number("Mg") => |row| number(row.magnesium, 1),
]);

catalogue!(Styles, styles, StyleRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::text("Type") => |row| label(row.stype(), |value| value.as_str()),
    Column::text("Category") => |row| text(row.category.as_deref()),
    Column::text("Number") => |row| text(row.category_number.as_deref()),
    Column::text("Letter") => |row| text(row.style_letter.as_deref()),
    Column::text("Guide") => |row| text(row.style_guide.as_deref()),
]);

catalogue!(Equipments, equipments, EquipmentRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::number("Mash Tun") => |row| number(row.mash_tun_volume_l, 1),
    Column::number("Kettle") => |row| number(row.kettle_volume_l, 1),
    Column::number("Fermenter") => |row| number(row.fv_volume_l, 1),
]);

// The three profiles a recipe refers to.  The Qt application edits these in dialogs rather than
// listing them, so these columns are what the dialogs show at the top rather than a ported table.
catalogue!(Mashes, mashes, MashRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::number("Grain Temp") => |row| number(row.grain_temp, 1),
    Column::number("Sparge Temp") => |row| number(row.sparge_temp, 1),
    Column::number("Sparge pH") => |row| number(row.ph, 2),
]);

catalogue!(Boils, boils, BoilRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::number("Pre-boil Size") => |row| number(row.pre_boil_size_l, 1),
    Column::text("Notes") => |row| text(row.notes.as_deref()),
]);

catalogue!(Fermentations, fermentations, FermentationRow, [
    Column::text("Name") => |row| text(row.name.as_deref()),
    Column::text("Description") => |row| text(row.description.as_deref()),
    Column::text("Notes") => |row| text(row.notes.as_deref()),
]);
