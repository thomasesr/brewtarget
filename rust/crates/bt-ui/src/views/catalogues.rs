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
use super::table::{Align, CatalogueTable, Cells, Column, Failed, Loading};
use crate::Client;

/// Build a component that fetches one catalogue and shows it as a table.
///
/// The ten of them differ only in what they fetch and what they show, so they are written as
/// ten descriptions rather than ten components.
macro_rules! catalogue {
    ($component:ident, $fetch:ident, $row:ty, $table:literal, [$($column:literal: $align:expr => $cell:expr),+ $(,)?]) => {
        #[component]
        pub fn $component(which: super::Which) -> Element {
            let client = use_context::<Client>();
            let rows = use_resource(move || {
                let client = client.clone();
                async move { client.$fetch().await }
            });

            let columns: Vec<Column> = vec![$(Column::of($table, $column, $align)),+];
            match &*rows.read_unchecked() {
                None => rsx! { Loading {} },
                Some(Err(error)) => rsx! { Failed { message: error.to_string() } },
                Some(Ok(found)) => {
                    let rows: Vec<(i32, Cells)> = found
                        .iter()
                        // A row marked deleted is kept so that recipes referring to it still
                        // resolve, and is hidden from the catalogue -- as `BtTableModel` hides it.
                        .filter(|row: &&$row| !row.deleted.unwrap_or(false))
                        .map(|row| (row.id, vec![$({ let cell: fn(&$row) -> String = $cell; cell(row) }),+]))
                        .collect();
                    rsx! { CatalogueTable { which, columns, rows } }
                }
            }
        }
    };
}

catalogue!(Fermentables, fermentables, FermentableRow, "fermentable", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "ftype": Align::Start => |row| label(row.ftype(), |value| value.display_name()),
    "fine_grind_yield_pct": Align::End => |row| percent(row.fine_grind_yield_pct),
    "color_lovibond": Align::End => |row| color_srm(row.color_lovibond),
]);

catalogue!(Hops, hops, HopRow, "hop", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "form": Align::Start => |row| label(row.form(), |value| value.display_name()),
    "year": Align::Start => |row| text(row.year.as_deref()),
    "alpha_pct": Align::End => |row| percent(row.alpha_pct),
]);

catalogue!(Miscs, miscs, MiscRow, "misc", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "mtype": Align::Start => |row| label(row.mtype(), |value| value.display_name()),
]);

catalogue!(Yeasts, yeasts, YeastRow, "yeast", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "laboratory": Align::Start => |row| text(row.laboratory.as_deref()),
    "product_id": Align::Start => |row| text(row.product_id.as_deref()),
    "ytype": Align::Start => |row| label(row.ytype(), |value| value.display_name()),
    "form": Align::Start => |row| label(row.form(), |value| value.display_name()),
]);

catalogue!(Waters, waters, WaterRow, "water", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "calcium": Align::End => |row| number(row.calcium, 1),
    "bicarbonate": Align::End => |row| number(row.bicarbonate, 1),
    "sulfate": Align::End => |row| number(row.sulfate, 1),
    "chloride": Align::End => |row| number(row.chloride, 1),
    "sodium": Align::End => |row| number(row.sodium, 1),
    "magnesium": Align::End => |row| number(row.magnesium, 1),
]);

catalogue!(Styles, styles, StyleRow, "style", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "stype": Align::Start => |row| label(row.stype(), |value| value.display_name()),
    "category": Align::Start => |row| text(row.category.as_deref()),
    "category_number": Align::Start => |row| text(row.category_number.as_deref()),
    "style_letter": Align::Start => |row| text(row.style_letter.as_deref()),
    "style_guide": Align::Start => |row| text(row.style_guide.as_deref()),
]);

catalogue!(Equipments, equipments, EquipmentRow, "equipment", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "mash_tun_volume_l": Align::End => |row| number(row.mash_tun_volume_l, 1),
    "kettle_volume_l": Align::End => |row| number(row.kettle_volume_l, 1),
    "fv_volume_l": Align::End => |row| number(row.fv_volume_l, 1),
]);

// The three profiles a recipe refers to.  The Qt application edits these in dialogs rather than
// listing them, so these columns are what the dialogs show at the top rather than a ported table.
catalogue!(Mashes, mashes, MashRow, "mash", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "grain_temp": Align::End => |row| number(row.grain_temp, 1),
    "sparge_temp": Align::End => |row| number(row.sparge_temp, 1),
    "ph": Align::End => |row| number(row.ph, 2),
]);

catalogue!(Boils, boils, BoilRow, "boil", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "pre_boil_size_l": Align::End => |row| number(row.pre_boil_size_l, 1),
    "notes": Align::Start => |row| text(row.notes.as_deref()),
]);

catalogue!(Fermentations, fermentations, FermentationRow, "fermentation", [
    "name": Align::Start => |row| text(row.name.as_deref()),
    "description": Align::Start => |row| text(row.description.as_deref()),
    "notes": Align::Start => |row| text(row.notes.as_deref()),
]);

/// Whichever catalogue the address names.
#[component]
pub fn Catalogue(which: super::Which) -> Element {
    use super::Which;
    rsx! {
        h2 { "{which.title()}" }
        match which {
            Which::Fermentables => rsx! { Fermentables { which } },
            Which::Hops => rsx! { Hops { which } },
            Which::Miscs => rsx! { Miscs { which } },
            Which::Yeasts => rsx! { Yeasts { which } },
            Which::Waters => rsx! { Waters { which } },
            Which::Styles => rsx! { Styles { which } },
            Which::Equipments => rsx! { Equipments { which } },
            Which::Mashes => rsx! { Mashes { which } },
            Which::Boils => rsx! { Boils { which } },
            Which::Fermentations => rsx! { Fermentations { which } },
        }
    }
}
