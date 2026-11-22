// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/steps.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Columns ported from src/qtModels/tableModels/{Mash,Boil,Fermentation}StepTableModel.cpp

//! What is done to a recipe: its mash, its boil, its fermentation.
//!
//! The last three of the seven tabs in `tabWidget_ingredients`.  They sit beside the
//! ingredients rather than with the profiles because these are *this* recipe's steps -- the
//! schedule it is actually brewed to.

use bt_model::recipe::Recipe;
use dioxus::prelude::*;

use super::format::{NOTHING, label, number, text};
use super::table::{Align, Cells, Column, DataTable};

/// A temperature, or a dash.
fn temperature(value: Option<f64>) -> String {
    number(value, 1)
}

/// A length of time in minutes.
fn minutes(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.0}"),
        None => NOTHING.to_owned(),
    }
}

/// Which step of the schedule this is.
fn step_number(value: Option<i32>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => NOTHING.to_owned(),
    }
}

#[component]
pub fn MashSteps(recipe: Recipe) -> Element {
    let columns = vec![
        Column::of("mash_step", "step_number", Align::End),
        Column::of("mash_step", "name", Align::Start),
        Column::of("mash_step", "mstype", Align::Start),
        Column::of("mash_step", "amount_l", Align::End),
        Column::of("mash_step", "infuse_temp_c", Align::End),
        Column::of("mash_step", "step_temp_c", Align::End),
        Column::of("mash_step", "end_temp_c", Align::End),
        Column::of("mash_step", "ramp_time_mins", Align::End),
        Column::of("mash_step", "step_time_mins", Align::End),
    ];
    let rows: Vec<Cells> = recipe
        .mash_steps()
        .iter()
        .map(|step| {
            vec![
                step_number(step.step_number),
                text(step.name.as_deref()),
                label(step.mstype(), |value| value.display_name()),
                number(step.amount_l, 2),
                temperature(step.infuse_temp_c),
                temperature(step.step_temp_c),
                temperature(step.end_temp_c),
                minutes(step.ramp_time_mins),
                minutes(step.step_time_mins),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}

#[component]
pub fn BoilSteps(recipe: Recipe) -> Element {
    let columns = vec![
        Column::of("boil_step", "step_number", Align::End),
        Column::of("boil_step", "name", Align::Start),
        Column::of("boil_step", "step_time_mins", Align::End),
        Column::of("boil_step", "start_temp_c", Align::End),
        Column::of("boil_step", "ramp_time_mins", Align::End),
        Column::of("boil_step", "end_temp_c", Align::End),
        Column::of("boil_step", "start_gravity_sg", Align::End),
        Column::of("boil_step", "end_gravity_sg", Align::End),
        Column::of("boil_step", "chilling_type", Align::Start),
    ];
    let rows: Vec<Cells> = recipe
        .boil_steps()
        .iter()
        .map(|step| {
            vec![
                step_number(step.step_number),
                text(step.name.as_deref()),
                minutes(step.step_time_mins),
                temperature(step.start_temp_c),
                minutes(step.ramp_time_mins),
                temperature(step.end_temp_c),
                number(step.start_gravity_sg, 3),
                number(step.end_gravity_sg, 3),
                label(step.chilling_type(), |value| value.display_name()),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}

#[component]
pub fn FermentationSteps(recipe: Recipe) -> Element {
    let columns = vec![
        Column::of("fermentation_step", "step_number", Align::End),
        Column::of("fermentation_step", "name", Align::Start),
        Column::of("fermentation_step", "step_time_mins", Align::End),
        Column::of("fermentation_step", "start_temp_c", Align::End),
        Column::of("fermentation_step", "end_temp_c", Align::End),
        Column::of("fermentation_step", "start_gravity_sg", Align::End),
        Column::of("fermentation_step", "end_gravity_sg", Align::End),
        Column::of("fermentation_step", "free_rise", Align::Start),
        Column::of("fermentation_step", "vessel", Align::Start),
    ];
    let rows: Vec<Cells> = recipe
        .fermentation_steps()
        .iter()
        .map(|step| {
            vec![
                step_number(step.step_number),
                text(step.name.as_deref()),
                minutes(step.step_time_mins),
                temperature(step.start_temp_c),
                temperature(step.end_temp_c),
                number(step.start_gravity_sg, 3),
                number(step.end_gravity_sg, 3),
                match step.free_rise {
                    Some(true) => "Yes".to_owned(),
                    Some(false) => "No".to_owned(),
                    None => NOTHING.to_owned(),
                },
                text(step.vessel.as_deref()),
            ]
        })
        .collect();
    rsx! { DataTable { columns, rows } }
}
