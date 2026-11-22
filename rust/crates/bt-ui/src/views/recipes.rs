// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/recipes.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Columns ported from src/qtModels/tableModels/RecipeTableModel.cpp

//! Every recipe there is.
//!
//! The one table whose rows are worth clicking, so it is written out rather than built by the
//! `catalogue!` macro the ingredient tables use.

use std::collections::BTreeMap;

use dioxus::prelude::*;

use super::format::{NOTHING, text};
use super::table::{Failed, Loading};
use crate::Client;
use crate::route::Route;

/// A batch size with its unit, or a dash.
fn batch_size(litres: Option<f64>) -> String {
    match litres {
        Some(litres) => format!("{litres:.1} L"),
        None => NOTHING.to_owned(),
    }
}

#[component]
pub fn Recipes() -> Element {
    let client = use_context::<Client>();
    let recipes = use_resource({
        let client = client.clone();
        move || {
            let client = client.clone();
            async move { client.recipe_rows().await }
        }
    });

    // A recipe row names its style by key.  The styles are fetched once and looked up, rather
    // than loading each recipe in full just to put a word in a column.
    let styles = use_resource(move || {
        let client = client.clone();
        async move {
            client.styles().await.map(|styles| {
                styles.into_iter().filter_map(|style| Some((style.id, style.name?))).collect::<BTreeMap<_, _>>()
            })
        }
    });
    let style_names = styles.read_unchecked().clone().and_then(Result::ok).unwrap_or_default();

    match &*recipes.read_unchecked() {
        None => rsx! { Loading {} },
        Some(Err(error)) => rsx! { Failed { message: error.to_string() } },
        Some(Ok(found)) => {
            let mut rows: Vec<_> = found.iter().filter(|row| !row.deleted.unwrap_or(false)).cloned().collect();
            // Alphabetical, which is how the Qt tree lists them and how anyone looking for one
            // expects to find it.
            rows.sort_by(|left, right| {
                left.name.as_deref().unwrap_or_default().cmp(right.name.as_deref().unwrap_or_default())
            });

            if rows.is_empty() {
                return rsx! { p { class: "bt-empty", "No recipes yet." } };
            }

            rsx! {
                table { class: "bt-table",
                    thead {
                        tr {
                            th { class: "bt-cell", "Name" }
                            th { class: "bt-cell", "Brewer" }
                            th { class: "bt-cell", "Style" }
                            th { class: "bt-number", "Batch" }
                        }
                    }
                    tbody {
                        for row in rows {
                            tr { key: "{row.id}",
                                // The name is a link rather than the whole row being clickable:
                                // a row that only responds to a click cannot be reached by
                                // keyboard, and a link can be opened in a new tab.
                                td { class: "bt-cell",
                                    Link { to: Route::Recipe { id: row.id }, class: "bt-link",
                                        "{text(row.name.as_deref())}"
                                    }
                                }
                                td { class: "bt-cell", "{text(row.brewer.as_deref())}" }
                                td { class: "bt-cell",
                                    "{row.style_id.and_then(|id| style_names.get(&id)).map(String::as_str).unwrap_or(NOTHING)}"
                                }
                                td { class: "bt-number", "{batch_size(row.batch_size)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
