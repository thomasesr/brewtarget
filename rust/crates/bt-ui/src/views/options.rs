// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/options.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from ui/optionsDialog.ui

//! What the brewer has chosen.
//!
//! The first two tabs of `optionsDialog.ui`: which units to show each quantity in, and which
//! formulae to use.  The others -- language, the database engine, logging, snapshots -- are
//! either settled by how this is deployed or not ported; see `PLAN.md`.

use std::collections::BTreeMap;

use bt_measure::UnitSystem;
use dioxus::prelude::*;
use serde_json::{Map, Value, json};

use super::table::{Failed, Loading};
use crate::Client;

/// Which tab is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Units,
    Formulas,
}

/// The physical quantities worth offering a choice for, in the order the dialog has them.
///
/// Not every quantity: bitterness is only ever IBU and time only ever minutes, so there is
/// nothing to choose.  A quantity with one system is left out rather than shown as a list of
/// one.
fn choosable_quantities() -> Vec<(String, Vec<&'static str>)> {
    let mut by_quantity: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for system in UnitSystem::all() {
        let systems = by_quantity.entry(system.physical_quantity().to_owned()).or_default();
        if !systems.contains(&system.system_of_measurement()) {
            systems.push(system.system_of_measurement());
        }
    }
    by_quantity.into_iter().filter(|(_, systems)| systems.len() > 1).collect()
}

#[component]
pub fn Options() -> Element {
    let client = use_context::<Client>();
    let stored = use_resource(move || {
        let client = client.clone();
        async move { client.preferences().await }
    });
    let mut chosen = use_signal(Map::<String, Value>::new);
    let mut problem = use_signal(|| Option::<String>::None);
    let mut saved = use_signal(|| false);
    let mut tab = use_signal(|| Tab::Units);

    use_effect(move || {
        if let Some(Ok(found)) = &*stored.read_unchecked() {
            chosen.set(found.clone());
        }
    });

    match &*stored.read_unchecked() {
        None => return rsx! { Loading {} },
        Some(Err(error)) => return rsx! { Failed { message: error.to_string() } },
        Some(Ok(_)) => {}
    }

    let client = use_context::<Client>();
    let save = move |_| {
        let client = client.clone();
        let preferences = chosen();
        problem.set(None);
        saved.set(false);
        spawn(async move {
            match client.set_preferences(&preferences).await {
                Ok(_) => saved.set(true),
                Err(error) => problem.set(Some(error.to_string())),
            }
        });
    };

    let open = tab();
    rsx! {
        h2 { "Options" }
        div { class: "bt-tabs", role: "tablist",
            for (entry, title) in [(Tab::Units, "Units && Precision"), (Tab::Formulas, "Formulas")] {
                button {
                    key: "{title}",
                    r#type: "button",
                    role: "tab",
                    class: if entry == open { "bt-tab bt-chosen" } else { "bt-tab" },
                    aria_selected: "{entry == open}",
                    onclick: move |_| tab.set(entry),
                    "{title}"
                }
            }
        }

        div { class: "bt-tool",
            if let Some(message) = problem() {
                p { class: "bt-error", role: "alert", "{message}" }
            }
            if saved() {
                p { class: "bt-done", role: "status", "Saved." }
            }

            match open {
                Tab::Units => rsx! {
                    p { class: "bt-note",
                        "What to show each kind of measurement in.  Values are stored the same way
                         whichever is chosen -- this changes how they are shown, not what is kept."
                    }
                    div { class: "bt-fields",
                        for (quantity, systems) in choosable_quantities() {
                            UnitChoice {
                                key: "{quantity}",
                                quantity: quantity.clone(),
                                systems: systems.clone(),
                                chosen,
                            }
                        }
                    }
                },
                Tab::Formulas => rsx! {
                    div { class: "bt-fields",
                        Choice {
                            label: "IBU",
                            field: "ibu_formula",
                            options: bt_calc::ibu::Formula::ALL.iter().map(|formula| formula.as_str()).collect::<Vec<_>>(),
                            chosen,
                        }
                        Choice {
                            label: "Color",
                            field: "color_formula",
                            options: bt_calc::color::ColorFormula::ALL.iter().map(|formula| formula.as_str()).collect::<Vec<_>>(),
                            chosen,
                        }
                    }
                    h3 { class: "bt-group", "IBU Adjustments" }
                    p { class: "bt-note",
                        "How much of a hop's bitterness to credit when it is added somewhere the
                         formulae were not written for."
                    }
                    div { class: "bt-fields",
                        Percentage { label: "Mash Hop (%)", field: "mash_hop_pct", chosen }
                        Percentage { label: "First Wort (%)", field: "first_wort_hop_pct", chosen }
                    }
                },
            }

            div { class: "bt-actions",
                button { r#type: "button", class: "bt-primary", onclick: save, "Save" }
            }
        }
    }
}

/// Which system of measurement to show one quantity in.
#[component]
fn UnitChoice(quantity: String, systems: Vec<&'static str>, chosen: Signal<Map<String, Value>>) -> Element {
    let mut chosen = chosen;
    let current = chosen()
        .get("units")
        .and_then(Value::as_object)
        .and_then(|units| units.get(&quantity))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let id = format!("units-{quantity}");

    rsx! {
        div { class: "bt-field",
            label { r#for: "{id}", "{readable(&quantity)}" }
            select {
                id: "{id}",
                value: "{current}",
                onchange: move |event| {
                    let mut all = chosen();
                    let units = all.entry("units").or_insert_with(|| json!({}));
                    if let Some(units) = units.as_object_mut() {
                        if event.value().is_empty() {
                            units.remove(&quantity);
                        } else {
                            units.insert(quantity.clone(), json!(event.value()));
                        }
                    }
                    chosen.set(all);
                },
                option { value: "", "Default" }
                for system in systems.iter() {
                    option { key: "{system}", value: "{system}", "{readable(system)}" }
                }
            }
        }
    }
}

/// One choice from a fixed list.
#[component]
fn Choice(
    label: &'static str,
    field: &'static str,
    options: Vec<&'static str>,
    chosen: Signal<Map<String, Value>>,
) -> Element {
    let mut chosen = chosen;
    let current = chosen().get(field).and_then(Value::as_str).unwrap_or_default().to_owned();
    rsx! {
        div { class: "bt-field",
            label { r#for: "{field}", "{label}" }
            select {
                id: "{field}",
                value: "{current}",
                onchange: move |event| {
                    let mut all = chosen();
                    all.insert(field.to_owned(), json!(event.value()));
                    chosen.set(all);
                },
                for option in options.iter() {
                    option { key: "{option}", value: "{option}", "{readable(option)}" }
                }
            }
        }
    }
}

/// A percentage someone types in.
#[component]
fn Percentage(label: &'static str, field: &'static str, chosen: Signal<Map<String, Value>>) -> Element {
    let mut chosen = chosen;
    let current = chosen().get(field).and_then(Value::as_f64).unwrap_or(0.0);
    rsx! {
        div { class: "bt-field",
            label { r#for: "{field}", "{label}" }
            input {
                id: "{field}",
                r#type: "number",
                step: "any",
                value: "{current}",
                oninput: move |event| {
                    if let Ok(value) = event.value().trim().parse::<f64>() {
                        let mut all = chosen();
                        all.insert(field.to_owned(), json!(value));
                        chosen.set(all);
                    }
                },
            }
        }
    }
}

/// `UsCustomary` becomes `Us Customary`, `mibu` becomes `Mibu`.
///
/// These names come from the C++ enums, which are written for code rather than for reading.
/// The Qt dialog has its own list of labels; until that is extracted this at least puts spaces
/// in the right places.
fn readable(value: &str) -> String {
    let mut out = String::new();
    for (index, character) in value.chars().enumerate() {
        if index > 0 && character.is_uppercase() {
            out.push(' ');
        }
        if index == 0 {
            out.extend(character.to_uppercase());
        } else {
            out.push(character);
        }
    }
    out
}
