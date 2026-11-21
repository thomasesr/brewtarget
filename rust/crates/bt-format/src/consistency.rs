// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/consistency.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Cross-field checks: the arithmetic a schema cannot do.
//!
//! Whether a batch will fit in the kettle depends on three fields in different units; whether an
//! addition's step exists depends on a record somewhere else in the file.  JSON Schema can say
//! nothing about either.
//!
//! Not every such problem is fatal.  dotBeer 1.0.0 rejected a default batch size that would not
//! fit; 1.1.0 downgraded it to a warning, on the grounds that a manufacturer's nominal batch size
//! often assumes a shorter boil and the file is still perfectly readable.  So the severity
//! depends on the version the document declares, and the two fixtures that cover this differ in
//! nothing else at all.

use serde_json::Value;

use crate::references::ReferenceError;

/// How much a problem matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth telling the user about; the file still loads.
    Warning,
    /// The file should be rejected.
    Error,
}

/// Something wrong that a schema cannot express.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl From<ReferenceError> for Finding {
    fn from(error: ReferenceError) -> Self {
        Self { severity: Severity::Error, path: error.path, message: error.message }
    }
}

/// Print a volume without trailing zeroes, so 30 litres reads as "30" and not "30.0000".
fn trim(value: f64) -> String {
    let rounded = (value * 10_000.0).round() / 10_000.0;
    let text = format!("{rounded}");
    // Only strip zeroes after a decimal point: 60 must not become 6.
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}

/// The boil length assumed when working out whether a batch fits, in minutes.
const ASSUMED_BOIL_MINUTES: f64 = 60.0;

/// Read a `{"unit": …, "value": …}` measurement as litres.
///
/// The unit names are dotBeer's; mapping them onto ours is what `bt-measure` is for, so "gal"
/// here really is a US gallon rather than an imperial one.
fn volume_litres(measurement: &Value) -> Option<f64> {
    let value = measurement.get("value")?.as_f64()?;
    let unit_name = measurement.get("unit")?.as_str()?;
    let unit = dotbeer_volume_unit(unit_name)?;
    Some(unit.to_canonical(value).quantity)
}

/// Map a dotBeer volume unit name onto ours.
///
/// Taken from `DOT_BEER_VOLUME_UNIT_MAPPER` in `schema/formats.json`.
fn dotbeer_volume_unit(name: &str) -> Option<bt_measure::Unit> {
    let unit_name = match name {
        "ml" => "milliliters",
        "l" => "liters",
        "tsp" => "us_teaspoons",
        "tbsp" => "us_tablespoons",
        "floz" => "us_fluidOunces",
        "cup" => "us_cups",
        "pt" => "us_pints",
        "qt" => "us_quarts",
        "gal" => "us_gallons",
        "bbl" => "us_barrels",
        "itsp" => "imperial_teaspoons",
        "itbsp" => "imperial_tablespoons",
        "ifloz" => "imperial_fluidOunces",
        "icup" => "imperial_cups",
        "ipt" => "imperial_pints",
        "iqt" => "imperial_quarts",
        "igal" => "imperial_gallons",
        "ibbl" => "imperial_barrels",
        _ => return None,
    };
    bt_measure::Unit::by_name(unit_name)
}

/// The version a document declares, as a comparable triple.
fn declared_version(document: &Value) -> Option<(u32, u32, u32)> {
    let text = document.get("DotBeer")?.get("version")?.as_str()?;
    let mut parts = text.split('.').map(|part| part.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Whether a batch size that will not fit is fatal, which it was before 1.1.0.
fn oversized_batch_severity(document: &Value) -> Severity {
    match declared_version(document) {
        Some(version) if version >= (1, 1, 0) => Severity::Warning,
        _ => Severity::Error,
    }
}

/// Check that each equipment's default batch size can actually be brewed in its kettle.
///
/// The pre-boil volume needed is the batch size, plus whatever the kettle keeps, plus whatever
/// boils away in an hour.  A kettle exactly full is fine -- the comparison is strict, and one of
/// the fixtures ("Exactly Full") exists to say so.
pub fn check_batch_sizes(document: &Value) -> Vec<Finding> {
    let severity = oversized_batch_severity(document);
    let Some(equipments) = document.get("DotBeer").and_then(|root| root.get("equipments")).and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    for (index, equipment) in equipments.iter().enumerate() {
        let Some(kettle) = equipment.get("kettle") else { continue };
        let (Some(batch_size_l), Some(max_volume_l)) = (
            equipment.get("default_batch_size").and_then(volume_litres),
            kettle.get("max_volume").and_then(volume_litres),
        ) else {
            continue;
        };

        let loss_l = kettle.get("loss").and_then(volume_litres).unwrap_or(0.0);
        // With no boil-off rate recorded there is nothing to add for it, which is what the
        // "No Boil-Off Given" fixture checks.
        let boil_off_l = kettle
            .get("boil_rate_per_hour")
            .and_then(volume_litres)
            .map_or(0.0, |per_hour| per_hour * ASSUMED_BOIL_MINUTES / 60.0);

        let needed_l = batch_size_l + loss_l + boil_off_l;
        if needed_l > max_volume_l {
            let name = equipment.get("name").and_then(Value::as_str).unwrap_or("");
            findings.push(Finding {
                severity,
                path: format!("/DotBeer/equipments/{index}"),
                message: format!(
                    "({name:?}): default batch size plus kettle loss{} needs a pre-boil volume of {} l, \
                     which is more than kettle max_volume ({} l)",
                    if boil_off_l > 0.0 {
                        format!(" plus {} minutes' boil-off", trim(ASSUMED_BOIL_MINUTES))
                    } else {
                        String::new()
                    },
                    trim(needed_l),
                    trim(max_volume_l)
                ),
            });
        }
    }
    findings
}

/// Which collection holds the steps an addition stage refers to.
fn stage_owner(stage: &str) -> Option<(&'static str, &'static str, &'static str)> {
    // (the recipe's reference field, the collection it points into, the steps inside it)
    match stage {
        "add_to_mash" => Some(("mash_id", "mashes", "mash_steps")),
        "add_to_boil" => Some(("boil_id", "boils", "boil_steps")),
        "add_to_fermentation" => Some(("fermentation_id", "fermentations", "fermentation_steps")),
        _ => None, // add_to_package has no steps to index into
    }
}

/// Check that every addition's `step` names a step that exists.
///
/// An addition scheduled for step 3 of a single-step mash is a mistake, and so is one scheduled
/// for a fermentation the recipe does not have.
pub fn check_addition_steps(document: &Value) -> Vec<Finding> {
    let Some(root) = document.get("DotBeer") else { return Vec::new() };
    let Some(recipes) = root.get("recipes").and_then(Value::as_array) else { return Vec::new() };

    let find_by_local_id = |collection: &str, local_id: &str| -> Option<&Value> {
        root.get(collection)?
            .as_array()?
            .iter()
            .find(|record| record.get("local_id").and_then(Value::as_str) == Some(local_id))
    };

    let mut findings = Vec::new();
    for (recipe_index, recipe) in recipes.iter().enumerate() {
        let recipe_name = recipe.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(ingredients) = recipe.get("ingredients").and_then(Value::as_object) else { continue };

        for (kind, additions) in ingredients {
            let Some(additions) = additions.as_array() else { continue };
            for (addition_index, addition) in additions.iter().enumerate() {
                let Some(schedule) = addition.get("schedule") else { continue };
                let (Some(stage), Some(step)) =
                    (schedule.get("stage").and_then(Value::as_str), schedule.get("step").and_then(Value::as_u64))
                else {
                    continue;
                };
                let Some((reference_field, collection, steps_field)) = stage_owner(stage) else { continue };
                let path = format!("/DotBeer/recipes/{recipe_index}/ingredients/{kind}/{addition_index}/schedule/step");

                let Some(owner_id) = recipe.get(reference_field).and_then(Value::as_str) else {
                    findings.push(Finding {
                        severity: Severity::Error,
                        path,
                        message: format!(
                            "schedule is for step {step} of {stage}, but recipe {recipe_name:?} has no {reference_field}"
                        ),
                    });
                    continue;
                };
                // A reference that does not resolve is reported by the reference checks, so
                // there is nothing to add here.
                let Some(owner) = find_by_local_id(collection, owner_id) else { continue };
                let step_count = owner.get(steps_field).and_then(Value::as_array).map_or(0, |steps| steps.len() as u64);
                if step == 0 || step > step_count {
                    let owner_name = owner.get("name").and_then(Value::as_str).unwrap_or(owner_id);
                    findings.push(Finding {
                        severity: Severity::Error,
                        path,
                        message: format!(
                            "schedule is for step {step} of {stage}, but {owner_name:?} only has {step_count} step(s)"
                        ),
                    });
                }
            }
        }
    }
    findings
}

/// Every cross-field check, in one pass.
pub fn check(document: &Value) -> Vec<Finding> {
    let mut findings = check_batch_sizes(document);
    findings.extend(check_addition_steps(document));
    findings.sort_by(|left, right| {
        right.severity.cmp(&left.severity).then(left.path.cmp(&right.path)).then(left.message.cmp(&right.message))
    });
    findings
}

/// Whether anything found is fatal.
pub fn has_errors(findings: &[Finding]) -> bool {
    findings.iter().any(|finding| finding.severity == Severity::Error)
}
