// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/table.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! A table of rows, and the two states that are not a table of rows.
//!
//! Replaces `BtTableModel` and the `QTableView`s built on it.  A Qt table model answers questions
//! about cells; this is handed the cells already worked out, because the thing being displayed is
//! text either way and keeping it that way means one table component rather than one per row type.

use dioxus::prelude::*;

use crate::route::Route;

/// How a column's values should sit in their cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Names, types, notes.
    Start,
    /// Quantities, so that the digits line up and can be compared down the column.
    End,
}

/// One column: what it is called and how it is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub header: &'static str,
    pub align: Align,
}

impl Column {
    pub fn text(header: &'static str) -> Self {
        Self { header, align: Align::Start }
    }

    pub fn number(header: &'static str) -> Self {
        Self { header, align: Align::End }
    }

    /// A column headed by whatever the application calls that column.
    ///
    /// Preferred over spelling the heading out here: the C++ named these, the names are
    /// extracted from it, and a second set written by hand would be a second thing to keep
    /// in step.  See `format::column_label`.
    pub fn of(table: &str, column: &'static str, align: Align) -> Self {
        Self { header: super::format::column_label(table, column), align }
    }

    pub fn class(&self) -> &'static str {
        match self.align {
            Align::Start => "bt-cell",
            Align::End => "bt-cell bt-number",
        }
    }
}

/// A row, already turned into the text that will be shown.
pub type Cells = Vec<String>;

/// Show a table of cells, or say there are none.
///
/// Plain: no links, no way in.  The ingredient tabs on a recipe use this, because an addition
/// is edited on the recipe rather than on its own.
#[component]
pub fn DataTable(columns: Vec<Column>, rows: Vec<Cells>) -> Element {
    if rows.is_empty() {
        return rsx! { p { class: "bt-empty", "Nothing here yet." } };
    }

    rsx! {
        table { class: "bt-table",
            thead {
                tr {
                    for column in columns.iter() {
                        th { class: column.class(), "{column.header}" }
                    }
                }
            }
            tbody {
                for (index, cells) in rows.iter().enumerate() {
                    tr { key: "{index}",
                        for (column, cell) in columns.iter().zip(cells) {
                            td { class: column.class(), "{cell}" }
                        }
                    }
                }
            }
        }
    }
}

/// Show a catalogue: the same table, with a way into each row and a way to add one.
///
/// Every row is keyed by what it is, so the link into its editor is part of the table rather
/// than something each of the ten catalogues has to remember to add.
#[component]
pub fn CatalogueTable(which: super::Which, columns: Vec<Column>, rows: Vec<(i32, Cells)>) -> Element {
    let add = rsx! {
        Link { to: Route::NewEntry { which }, class: "bt-primary bt-add", "New {which.singular()}" }
    };

    if rows.is_empty() {
        // An empty catalogue is a normal state, not a failure: a fresh database has none of
        // anything until the default content is loaded or a file is imported.
        return rsx! {
            p { class: "bt-empty", "Nothing here yet." }
            div { class: "bt-toolbar", {add} }
        };
    }

    rsx! {
        div { class: "bt-toolbar", {add} }
        table { class: "bt-table",
            thead {
                tr {
                    for column in columns.iter() {
                        th { class: column.class(), "{column.header}" }
                    }
                    th { class: "bt-cell", span { class: "bt-hidden", "Edit" } }
                }
            }
            tbody {
                for (id, cells) in rows.iter() {
                    tr { key: "{id}",
                        for (column, cell) in columns.iter().zip(cells) {
                            td { class: column.class(), "{cell}" }
                        }
                        td { class: "bt-cell",
                            Link { to: Route::EditEntry { which, id: *id }, class: "bt-link", "Edit" }
                        }
                    }
                }
            }
        }
    }
}

/// What to show while an answer is still on its way.
#[component]
pub fn Loading() -> Element {
    rsx! { p { class: "bt-loading", "Loading…" } }
}

/// What to show when the server could not answer.
///
/// The message is the server's own where there is one.  Nothing is swallowed: a table that
/// quietly shows nothing when the request failed is indistinguishable from an empty catalogue,
/// and those are very different things to a user wondering where their recipes went.
#[component]
pub fn Failed(message: String) -> Element {
    rsx! {
        p { class: "bt-error", role: "alert",
            "Could not load this: {message}"
        }
    }
}
