// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/editor.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Replaces the dialogs in ui/*Editor.ui

//! Changing a row.
//!
//! One editor for every table, built from the schema rather than written out ten times: the
//! columns, what they are called and what may go in them all come from `Editable`, so a column
//! cannot be added without a way to edit it appearing too.
//!
//! The Qt version has a dialog per entity in `ui/*Editor.ui`, each laid out by hand.  Those
//! layouts group related fields in ways this does not yet; what this has instead is every field,
//! for every table, which the hand-written ones do not.

use std::collections::BTreeMap;

use bt_model::tables::{BaseType, Editable, FormTab, editable_columns, form_for};
use dioxus::prelude::*;
use serde_json::{Map, Value};

use super::table::{Failed, Loading};
use crate::Client;

/// What the editor is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editing {
    /// Changing a row that exists.
    Existing(i32),
    /// Making one that does not.
    New,
}

/// Turn what was typed into what the column holds.
///
/// An empty box means the column is not set, which is a different thing from zero or from an
/// empty string -- almost every column in this schema is nullable and most of them are null.
fn parse(kind: BaseType, typed: &str) -> Result<Value, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(Value::Null);
    }
    match kind {
        BaseType::Bool => match typed {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            other => Err(format!("{other:?} is neither true nor false")),
        },
        BaseType::Int => typed.parse::<i64>().map(Value::from).map_err(|_| format!("{typed:?} is not a whole number")),
        BaseType::Double => typed.parse::<f64>().map(Value::from).map_err(|_| format!("{typed:?} is not a number")),
        BaseType::String | BaseType::Date => Ok(Value::String(typed.to_owned())),
    }
}

/// Show a stored value in a box someone can type in.
fn show(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// What kind of control a column needs.
fn is_long_text(field: &Editable) -> bool {
    // The columns people write sentences in rather than words.
    field.kind == BaseType::String
        && matches!(field.column, "notes" | "description" | "ingredients" | "examples" | "best_for" | "use_for")
}

/// The rows of every table a foreign key on this one names, so they can be chosen from.
///
/// Fetched together rather than one picker at a time: a recipe names six other rows, and six
/// requests fired off independently would have the screen filling in piecemeal.
#[derive(Debug, Clone, Default, PartialEq)]
struct Choosable(BTreeMap<String, Vec<(i32, String)>>);

impl Choosable {
    async fn fetch(client: &Client, fields: &[Editable]) -> Self {
        let mut found = BTreeMap::new();
        for table in fields.iter().filter_map(|field| field.references) {
            if found.contains_key(table) {
                continue;
            }
            // A table that cannot be read leaves its picker empty rather than stopping the
            // whole editor: every other field is still worth being able to change.
            let rows = client.rows(table).await.unwrap_or_default();
            let mut named: Vec<(i32, String)> = rows
                .iter()
                .filter(|row| !row.get("deleted").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|row| {
                    let id = i32::try_from(row.get("id")?.as_i64()?).ok()?;
                    let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
                    // A row with no name is still choosable; its key is all there is to go on.
                    Some((id, if name.is_empty() { format!("#{id}") } else { name.to_owned() }))
                })
                .collect();
            named.sort_by(|left, right| left.1.cmp(&right.1));
            found.insert(table.to_owned(), named);
        }
        Self(found)
    }

    fn of(&self, table: &str) -> &[(i32, String)] {
        self.0.get(table).map(Vec::as_slice).unwrap_or_default()
    }
}

/// The editor for one row of one table.
///
/// `fixed` are columns set by whoever opened the editor rather than by whoever is using it --
/// the recipe a hop addition belongs to, say.  They are written with the row and not shown.
#[component]
pub fn Editor(
    table: String,
    editing: Editing,
    #[props(default)] fixed: Vec<(String, Value)>,
    on_done: EventHandler<Option<i32>>,
) -> Element {
    let client = use_context::<Client>();

    // How the Qt application arranges this editor: which tabs, which groups, what order,
    // what each field is called there.  A table it has no dialog for -- the recipe
    // additions, say -- falls back to the schema's own order.
    let form = arrangement(&table, &fixed);
    let fields: Vec<Editable> =
        form.iter().flat_map(|tab| tab.fields.iter()).filter_map(|field| field.editable).collect();
    let mut tab = use_signal(usize::default);

    let choosable = use_resource({
        let (client, fields) = (client.clone(), fields.clone());
        move || {
            let (client, fields) = (client.clone(), fields.clone());
            async move { Choosable::fetch(&client, &fields).await }
        }
    });

    // What is in the boxes, by column.  Separate from what was loaded so that an edit can be
    // abandoned, and so that what someone typed survives a failed save.
    let mut typed = use_signal(BTreeMap::<String, String>::new);
    let mut loaded = use_signal(Map::<String, Value>::new);
    let mut problem = use_signal(|| Option::<String>::None);
    let mut saving = use_signal(|| false);
    let mut confirming = use_signal(|| false);

    let existing = use_resource({
        let client = client.clone();
        let table = table.clone();
        move || {
            let (client, table) = (client.clone(), table.clone());
            async move {
                match editing {
                    Editing::Existing(id) => client.row(&table, id).await.map(Some),
                    Editing::New => Ok(None),
                }
            }
        }
    });

    // Fill the boxes once, when the row arrives.  `use_effect` rather than doing it during
    // render: writing to a signal while rendering would start the render again.
    use_effect(move || {
        if let Some(Ok(Some(row))) = &*existing.read_unchecked() {
            loaded.set(row.clone());
            typed.set(row.iter().map(|(column, value)| (column.clone(), show(Some(value)))).collect());
        }
    });

    match &*existing.read_unchecked() {
        None => return rsx! { Loading {} },
        Some(Err(error)) => return rsx! { Failed { message: error.to_string() } },
        Some(Ok(_)) => {}
    }

    let remove = {
        // Each handler gets its own copy, since both outlive the body of this function.
        let (client, table) = (client.clone(), table.clone());
        move |_| {
            let Editing::Existing(id) = editing else { return };
            let (client, table) = (client.clone(), table.clone());
            problem.set(None);
            saving.set(true);
            spawn(async move {
                let deleted = client.delete_row(&table, id).await;
                saving.set(false);
                match deleted {
                    Ok(()) => on_done.call(None),
                    Err(error) => problem.set(Some(error.to_string())),
                }
            });
        }
    };

    let read_only_columns: Vec<&'static str> = form
        .iter()
        .flat_map(|page| page.fields.iter())
        .filter(|shown| shown.read_only)
        .filter_map(|shown| shown.editable.map(|field| field.column))
        .collect();

    let save = {
        let fields = fields.clone();
        let read_only_columns = read_only_columns.clone();
        move |event: FormEvent| {
            // Without this the browser navigates away and the whole application reloads.
            event.prevent_default();
            let (client, table) = (client.clone(), table.clone());
            // Everything typed, turned back into columns.  A box left empty clears the column.
            let mut row = loaded();
            let mut rejected = None;
            for field in fields.iter() {
                if read_only_columns.contains(&field.column) {
                    // `WhenToWriteField::Never` on the Qt dialog: shown so it can be read,
                    // never written back, because something else works it out.
                    continue;
                }
                let text = typed().get(field.column).cloned().unwrap_or_default();
                match parse(field.kind, &text) {
                    Ok(value) => {
                        row.insert(field.column.to_owned(), value);
                    }
                    Err(message) => {
                        rejected = Some(format!("{}: {message}", field.label));
                        break;
                    }
                }
            }
            if let Some(message) = rejected {
                problem.set(Some(message));
                return;
            }

            problem.set(None);
            saving.set(true);
            spawn(async move {
                let stored = match editing {
                    Editing::Existing(id) => client.put_row(&table, id, &row).await,
                    Editing::New => client.post_row(&table, &row).await,
                };
                saving.set(false);
                match stored {
                    Ok(stored) => {
                        let id = stored.get("id").and_then(Value::as_i64).and_then(|id| i32::try_from(id).ok());
                        on_done.call(id);
                    }
                    // The boxes keep what was typed, so nothing is lost to a failed save.
                    Err(error) => problem.set(Some(error.to_string())),
                }
            });
        }
    };

    rsx! {
        form { class: "bt-editor", onsubmit: save,
            if let Some(message) = problem() {
                p { class: "bt-error", role: "alert", "{message}" }
            }
            if form.len() > 1 {
                div { class: "bt-tabs", role: "tablist",
                    for (index, page) in form.iter().enumerate() {
                        button {
                            key: "{page.title}",
                            r#type: "button",
                            role: "tab",
                            class: if index == tab() { "bt-tab bt-chosen" } else { "bt-tab" },
                            aria_selected: "{index == tab()}",
                            onclick: move |_| tab.set(index),
                            "{page.title}"
                        }
                    }
                }
            }

            for (index, page) in form.iter().enumerate() {
                // Every tab stays in the document, hidden rather than removed, so that what
                // was typed on one survives looking at another before saving.
                div {
                    key: "{page.title}",
                    role: "tabpanel",
                    aria_label: "{page.title}",
                    hidden: index != tab(),
                    for section in page.sections() {
                        if let Some(heading) = section.heading {
                            h3 { class: "bt-group", "{heading}" }
                        }
                        div { class: "bt-fields",
                            for shown in section.fields.iter().copied() {
                                if let Some(field) = shown.editable {
                                    Field {
                                        key: "{field.column}",
                                        field,
                                        label: shown.label,
                                        read_only: shown.read_only,
                                        value: typed().get(field.column).cloned().unwrap_or_default(),
                                        choices: choosable()
                                            .unwrap_or_default()
                                            .of(field.references.unwrap_or_default())
                                            .to_vec(),
                                        on_change: move |text| {
                                            typed.write().insert(field.column.to_owned(), text);
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "bt-actions",
                button { r#type: "submit", class: "bt-primary", disabled: saving(),
                    if saving() { "Saving…" } else { "Save" }
                }
                button { r#type: "button", class: "bt-secondary", onclick: move |_| on_done.call(None), "Cancel" }
                if matches!(editing, Editing::Existing(_)) {
                    // Asked for twice, because this is the one action here that cannot be
                    // undone from this screen.
                    if confirming() {
                        span { class: "bt-confirm",
                            "Delete this?"
                            button { r#type: "button", class: "bt-danger", onclick: remove, "Yes, delete" }
                            button {
                                r#type: "button",
                                class: "bt-secondary",
                                onclick: move |_| confirming.set(false),
                                "No"
                            }
                        }
                    } else {
                        button {
                            r#type: "button",
                            class: "bt-danger bt-push",
                            onclick: move |_| confirming.set(true),
                            "Delete"
                        }
                    }
                }
            }
        }
    }
}

/// One labelled control.
#[component]
fn Field(
    field: Editable,
    /// What the dialog calls this field, which is not always what the column is called.
    label: &'static str,
    /// Shown but never written back.
    read_only: bool,
    value: String,
    /// The rows this field may name, where it names a row of another table.
    choices: Vec<(i32, String)>,
    on_change: EventHandler<String>,
) -> Element {
    let id = format!("field-{}", field.column);

    rsx! {
        div { class: if is_long_text(&field) { "bt-field bt-wide" } else { "bt-field" },
            label { r#for: "{id}", "{label}" }
            if read_only {
                // Disabled rather than absent: the dialog shows these, and what they say is
                // worth reading even though it cannot be changed here.
                input { id: "{id}", value: "{value}", disabled: true, readonly: true }
            } else if field.references.is_some() {
                // A row of another table: chosen by its name, stored as its key.
                select {
                    id: "{id}",
                    value: "{value}",
                    onchange: move |event| on_change.call(event.value()),
                    option { value: "", "—" }
                    for (key, name) in choices.iter() {
                        option { key: "{key}", value: "{key}", "{name}" }
                    }
                }
            } else if !field.choices.is_empty() {
                // An enumerated column: the stored value goes in, the shown name comes out.
                select {
                    id: "{id}",
                    value: "{value}",
                    onchange: move |event| on_change.call(event.value()),
                    // Nullable, like almost every column here, so "not set" has to be choosable.
                    option { value: "", "—" }
                    for (stored, shown) in field.choices.iter() {
                        option { key: "{stored}", value: "{stored}", "{shown}" }
                    }
                }
            } else if field.kind == BaseType::Bool {
                select {
                    id: "{id}",
                    value: "{value}",
                    onchange: move |event| on_change.call(event.value()),
                    option { value: "", "—" }
                    option { value: "true", "Yes" }
                    option { value: "false", "No" }
                }
            } else if is_long_text(&field) {
                textarea { id: "{id}", rows: "3", value: "{value}", oninput: move |event| on_change.call(event.value()) }
            } else {
                input {
                    id: "{id}",
                    r#type: match field.kind {
                        BaseType::Int | BaseType::Double => "number",
                        BaseType::Date => "date",
                        _ => "text",
                    },
                    step: if field.kind == BaseType::Double { "any" },
                    value: "{value}",
                    oninput: move |event| on_change.call(event.value()),
                }
            }
        }
    }
}

/// The editor as a screen of its own, with somewhere to go when it is finished.
#[component]
pub fn Screen(which: super::Which, editing: Editing) -> Element {
    let navigator = use_navigator();
    let back = crate::route::Route::Catalogue { which };

    rsx! {
        h2 {
            match editing {
                Editing::New => format!("New {}", which.singular()),
                Editing::Existing(_) => format!("Edit {}", which.singular()),
            }
        }
        Editor {
            table: which.table().to_owned(),
            editing,
            // Saved or abandoned, the way back is the same: the catalogue it came from, which
            // reloads and so shows the change.
            on_done: move |_| {
                navigator.push(back.clone());
            },
        }
    }
}

/// One field as the editor shows it: the dialog's label and the column behind it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub label: &'static str,
    pub group: Option<&'static str>,
    pub read_only: bool,
    /// `None` where the dialog shows something no column holds, which is nothing to edit.
    pub editable: Option<Editable>,
}

/// A run of consecutive fields under one heading.
pub struct Section {
    pub heading: Option<&'static str>,
    pub fields: Vec<Shown>,
}

/// One tab of an editor.
pub struct Page {
    pub title: &'static str,
    pub fields: Vec<Shown>,
}

impl Page {
    /// The tab's fields, split where the heading above them changes.
    ///
    /// Consecutive rather than gathered: the dialog's order is what is being reproduced, and
    /// collecting every field of a group together would reorder the ones between them.
    pub fn sections(&self) -> Vec<Section> {
        let mut sections: Vec<Section> = Vec::new();
        for field in &self.fields {
            match sections.last_mut() {
                Some(section) if section.heading == field.group => section.fields.push(*field),
                _ => sections.push(Section { heading: field.group, fields: vec![*field] }),
            }
        }
        sections
    }
}

/// How to arrange this table's editor.
///
/// The Qt dialog's arrangement where there is one, and the schema's own order where there is
/// not -- the recipe addition tables have no dialog of their own, because the Qt application
/// edits those in the recipe's ingredient tables rather than in a window.
fn arrangement(table: &str, fixed: &[(String, Value)]) -> Vec<Page> {
    let is_fixed = |column: &str| fixed.iter().any(|(fixed, _)| fixed == column);
    let editable = |column: &str| editable_columns(table).iter().find(|field| field.column == column).copied();

    match form_for(table) {
        Some(form) => from_dialog(table, form, &is_fixed, &editable),
        None => vec![Page {
            title: "Details",
            fields: editable_columns(table)
                .iter()
                .filter(|field| !is_fixed(field.column))
                .map(|field| Shown { label: field.label, group: None, read_only: false, editable: Some(*field) })
                .collect(),
        }],
    }
}

/// The Qt dialog's tabs, with anything it does not cover added on the end.
fn from_dialog(
    table: &str,
    form: &'static [FormTab],
    is_fixed: &impl Fn(&str) -> bool,
    editable: &impl Fn(&str) -> Option<Editable>,
) -> Vec<Page> {
    let mut pages: Vec<Page> = form
        .iter()
        .map(|tab| Page {
            title: tab.title,
            fields: tab
                .fields
                .iter()
                .filter(|field| !is_fixed(field.column))
                .map(|field| Shown {
                    label: field.label,
                    group: field.group,
                    read_only: field.read_only,
                    editable: editable(field.column),
                })
                .collect(),
        })
        .filter(|page| !page.fields.is_empty())
        .collect();

    // A column the dialog does not show is still a column, and this is still the only way to
    // change one.  They go on a tab of their own rather than among the ones the dialog
    // arranged, so that what the dialog does show reads as the dialog has it.
    let shown: Vec<&str> = form.iter().flat_map(|tab| tab.fields.iter()).map(|field| field.column).collect();
    let rest: Vec<Shown> = leftovers(table, &shown, is_fixed);
    if !rest.is_empty() {
        pages.push(Page { title: "Other", fields: rest });
    }
    pages
}

/// Every editable column of the table that the dialog leaves out.
fn leftovers(table: &str, shown: &[&str], is_fixed: &impl Fn(&str) -> bool) -> Vec<Shown> {
    editable_columns(table)
        .iter()
        .filter(|field| !shown.contains(&field.column) && !is_fixed(field.column))
        .map(|field| Shown { label: field.label, group: None, read_only: false, editable: Some(*field) })
        .collect()
}
