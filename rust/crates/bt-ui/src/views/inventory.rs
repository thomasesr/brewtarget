// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/inventory.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/inventoryWindow.ui

//! What is in the store cupboard.
//!
//! `inventoryWindow.ui` has a tab per kind of ingredient, each a tree of what is on hand.  Here
//! each ingredient is a row, and the lots it is held in sit under it -- which is the same
//! information, and is what a tree was doing.

use dioxus::prelude::*;
use serde_json::Value;

use super::format::{NOTHING, number, text};
use super::table::{Failed, Loading};
use crate::Client;

/// Which tab is open.  The five in `inventoryWindow.ui`, less Salts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Fermentables,
    Hops,
    Miscs,
    Yeasts,
}

impl Which {
    const ALL: [Self; 4] = [Self::Fermentables, Self::Hops, Self::Miscs, Self::Yeasts];

    fn title(self) -> &'static str {
        match self {
            Self::Fermentables => "Fermentables",
            Self::Hops => "Hops",
            Self::Miscs => "Miscellaneous",
            Self::Yeasts => "Yeasts",
        }
    }

    /// What `/api/inventory/{kind}` calls it.
    fn kind(self) -> &'static str {
        match self {
            Self::Fermentables => "fermentable",
            Self::Hops => "hop",
            Self::Miscs => "misc",
            Self::Yeasts => "yeast",
        }
    }
}

#[component]
pub fn Inventory() -> Element {
    let mut which = use_signal(|| Which::Fermentables);
    let open = which();

    rsx! {
        h2 { "Inventory" }
        div { class: "bt-tabs", role: "tablist",
            for entry in Which::ALL {
                button {
                    key: "{entry:?}",
                    role: "tab",
                    class: if entry == open { "bt-tab bt-chosen" } else { "bt-tab" },
                    aria_selected: "{entry == open}",
                    onclick: move |_| which.set(entry),
                    "{entry.title()}"
                }
            }
        }
        div { role: "tabpanel", aria_label: "{open.title()}",
            Holdings { kind: open.kind() }
        }
    }
}

/// What is on hand of one kind, with the lots each is held in.
#[component]
fn Holdings(kind: String) -> Element {
    let client = use_context::<Client>();
    let holdings = use_resource({
        let kind = kind.clone();
        move || {
            let (client, kind) = (client.clone(), kind.clone());
            async move { client.holdings(&kind).await }
        }
    });

    match &*holdings.read_unchecked() {
        None => rsx! { Loading {} },
        Some(Err(error)) => rsx! { Failed { message: error.to_string() } },
        Some(Ok(found)) if found.is_empty() => rsx! {
            p { class: "bt-empty",
                "Nothing has been bought of any of these.  Inventory is built from stock
                 purchases, which are recorded against each ingredient."
            }
        },
        Some(Ok(found)) => {
            let mut rows = found.clone();
            rows.sort_by_key(name_of);
            rsx! {
                table { class: "bt-table",
                    thead {
                        tr {
                            th { class: "bt-cell", "Name" }
                            th { class: "bt-number", "On Hand" }
                            th { class: "bt-cell", "Lots" }
                        }
                    }
                    tbody {
                        for holding in rows.iter() {
                            tr { key: "{holding.get(\"ingredient_id\").and_then(Value::as_i64).unwrap_or_default()}",
                                td { class: "bt-cell", "{name_of(holding)}" }
                                td { class: "bt-number",
                                    "{number(holding.get(\"quantity\").and_then(Value::as_f64), 3)} {unit_of(holding)}"
                                }
                                td { class: "bt-cell", "{lots_of(holding)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn name_of(holding: &serde_json::Map<String, Value>) -> String {
    text(holding.get("ingredient_name").and_then(Value::as_str))
}

fn unit_of(holding: &serde_json::Map<String, Value>) -> String {
    holding.get("unit").and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// A summary of the lots an ingredient is held in.
///
/// The Qt window puts these under the ingredient in a tree; a line saying how many there are and
/// how much is left in each says the same thing without one.
fn lots_of(holding: &serde_json::Map<String, Value>) -> String {
    let Some(lots) = holding.get("lots").and_then(Value::as_array) else { return NOTHING.to_owned() };
    if lots.is_empty() {
        return NOTHING.to_owned();
    }
    let remaining: Vec<String> = lots
        .iter()
        .map(|lot| {
            let left = lot.get("remaining").and_then(Value::as_f64).unwrap_or(0.0);
            let supplier = lot.get("supplier").and_then(Value::as_str).unwrap_or_default();
            if supplier.is_empty() { format!("{left:.3}") } else { format!("{left:.3} ({supplier})") }
        })
        .collect();
    format!("{} × {}", lots.len(), remaining.join(", "))
}
