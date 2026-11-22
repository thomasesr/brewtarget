// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/transfer.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Replaces the File > Import/Export menu in src/MainWindow.cpp

//! Getting a library in and out.
//!
//! The three interchange formats, in both directions.  Which one a file is in is worked out by
//! the server from its contents rather than from its name, so there is nothing to choose when
//! importing -- only when writing one out.

use dioxus::prelude::*;

use crate::Client;

/// The formats a library can be written out as, with what to call the file.
///
/// `.beer` first: it is the most expressive of the three and the only one that can hold
/// everything this application models.
const FORMATS: [(&str, &str, &str); 3] = [
    ("dot_beer", ".beer", "brewtarget.beer"),
    ("beer_json", "BeerJSON", "brewtarget.json"),
    ("beer_xml", "BeerXML", "brewtarget.xml"),
];

/// What happened to the last import.
#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Reading,
    Stored { format: String, rows: usize, recipes: usize },
    Failed(String),
}

#[component]
pub fn Transfer() -> Element {
    let client = use_context::<Client>();
    let mut outcome = use_signal(|| Option::<Outcome>::None);

    let choose = move |event: FormEvent| {
        let client = client.clone();
        let Some(file) = event.files().first().cloned() else { return };

        outcome.set(Some(Outcome::Reading));
        spawn(async move {
            let contents = match file.read_string().await {
                Ok(contents) => contents,
                Err(error) => {
                    outcome.set(Some(Outcome::Failed(format!("{} could not be read: {error}", file.name()))));
                    return;
                }
            };
            match client.import(&contents).await {
                Ok(summary) => outcome.set(Some(Outcome::Stored {
                    format: summary.format,
                    rows: summary.rows_written,
                    recipes: summary.recipe_ids.len(),
                })),
                Err(error) => outcome.set(Some(Outcome::Failed(error.to_string()))),
            }
        });
    };

    rsx! {
        h2 { "Import and Export" }

        section { class: "bt-transfer",
            h3 { "Import" }
            p { class: "bt-note",
                "A .beer, BeerJSON or BeerXML file.  Which it is, is worked out from what is in
                 it rather than from what it is called."
            }
            input { r#type: "file", accept: ".beer,.json,.xml", onchange: choose }

            match outcome() {
                None => rsx! {},
                Some(Outcome::Reading) => rsx! { p { class: "bt-loading", "Reading…" } },
                Some(Outcome::Stored { format, rows, recipes }) => rsx! {
                    p { class: "bt-done", role: "status",
                        "Read as {format}: {rows} rows stored, {recipes} of them recipes."
                    }
                },
                Some(Outcome::Failed(message)) => rsx! {
                    p { class: "bt-error", role: "alert", "{message}" }
                },
            }
        }

        section { class: "bt-transfer",
            h3 { "Export" }
            p { class: "bt-note", "Everything in the database, as one file." }
            div { class: "bt-actions",
                for (format, label, file_name) in FORMATS {
                    a {
                        key: "{format}",
                        class: "bt-secondary",
                        href: "{use_context::<Client>().export_url(format)}",
                        download: "{file_name}",
                        "{label}"
                    }
                }
            }
        }
    }
}
