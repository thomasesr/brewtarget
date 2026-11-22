// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/legacy.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/serialization/xml/XmlRecipeRecord.cpp, src/model/RecipeAdditionHop.cpp,
// src/model/RecipeAdditionMisc.cpp and src/model/RecipeAdditionYeast.cpp

//! Making sense of what BeerXML says in its own terms.
//!
//! BeerXML predates most of what the application now models.  It has no boil and no fermentation:
//! a recipe simply has a `BOIL_TIME` and a `PRIMARY_AGE`.  It has no addition stages either: a
//! hop has a `USE` of "Dry Hop" and a `TIME` that means something different for each use.
//!
//! The readers collect those fields as they go (see `MappedRecord::legacy`) and this is where
//! they become rows: a boil with three steps, a fermentation with up to three, a hop addition
//! with a stage and a step.  The C++ does the same work in two places -- the model constructors
//! for additions, `XmlRecipeRecord::normaliseAndStoreChildRecordsInDb` for the recipe -- because
//! of where the information is available to it; the result is the same either way.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::document::{Child, Nesting, Owned};
use crate::mapping::MappedRecord;

/// Minutes in a day, for BeerXML's fermentation ages.  `StepBase::setStepTime_days`.
const MINUTES_IN_A_DAY: f64 = 1440.0;

/// Fill in everything a record implies but does not say.
///
/// Called for every record as it is read, in the order the C++ does it: the record's own columns
/// first, then whatever has to be created around it.
pub fn expand(owned: &mut Owned) {
    match owned.record.table {
        "recipe" => expand_recipe(owned),
        "fermentable_in_recipe" => default_stage(&mut owned.record, "add_to_mash"),
        "hop_in_recipe" => expand_hop_addition(&mut owned.record),
        "misc_in_recipe" => expand_misc_addition(&mut owned.record),
        "yeast_in_recipe" => expand_yeast_addition(&mut owned.record),
        _ => {}
    }
    default_unit(&mut owned.record);
    // Whatever was not used here was calculated rather than recorded, and is worked out again
    // rather than read back.  Dropping it now means a record holds only what it is, which is what
    // everything downstream -- storing a row, writing a file back out -- deals in.
    owned.record.legacy = BTreeMap::new();
}

/// Give an addition a stage if the file did not.
///
/// Each kind of addition assumes the first stage at which it is usual to add that ingredient --
/// malt in the mash, hops in the boil, yeast at fermentation -- which is what the constructors in
/// `src/model/RecipeAddition*.cpp` do.
fn default_stage(record: &mut MappedRecord, stage: &str) {
    record.columns.entry("stage".to_owned()).or_insert_with(|| Value::String(stage.to_owned()));
}

/// The tables that record an amount, and what the ingredient they name is measured in.
///
/// `defaultMeasure` in each model class.  Water is the only thing measured by volume, and water
/// is not an addition.
const MEASURED: [(&str, &str); 8] = [
    ("fermentable_in_recipe", "Mass"),
    ("hop_in_recipe", "Mass"),
    ("misc_in_recipe", "Mass"),
    ("yeast_in_recipe", "Mass"),
    ("fermentable_stock_purchase", "Mass"),
    ("hop_stock_purchase", "Mass"),
    ("misc_stock_purchase", "Mass"),
    ("yeast_stock_purchase", "Mass"),
];

/// Say what an amount is measured in, where the file gave a number and no unit.
///
/// BeerXML has no element for it: an `AMOUNT` is in kilograms or litres because the standard says
/// so.  `IngredientAmount`'s constructor in `src/model/IngredientAmount.h` fills in the canonical
/// unit for whatever the ingredient is measured in, and the row it then stores has that unit in
/// it, so the same has to happen here or the column would be left empty.
fn default_unit(record: &mut MappedRecord) {
    let Some((_, measure)) = MEASURED.iter().find(|(table, _)| *table == record.table) else {
        return;
    };
    if !record.columns.contains_key("quantity") || record.columns.contains_key("unit") {
        return;
    }
    if let Some(unit) = bt_measure::Unit::canonical_for(measure) {
        record.columns.insert("unit".to_owned(), Value::String(unit.name().to_owned()));
    }
}

/// Reads a BeerXML "use" and says where in the brew the addition belongs.
///
/// `StageAndStep` in `RecipeAdditionHop.cpp` and `RecipeAdditionMisc.cpp`.
type StageAndStepFor = fn(&str) -> Option<(&'static str, Option<i64>)>;

/// Where a hop goes, given BeerXML's one word for it.
///
/// Ported from `stageAndStepFor` in `RecipeAdditionHop.cpp`.  The step numbers are positions
/// within the boil: a first wort hop goes in during the ramp-up, an aroma hop after the boil.
fn hop_stage_and_step(use_of: &str) -> Option<(&'static str, Option<i64>)> {
    Some(match use_of {
        "Mash" => ("add_to_mash", None),
        "First Wort" => ("add_to_boil", Some(1)),
        "Boil" => ("add_to_boil", Some(2)),
        "Aroma" => ("add_to_boil", Some(3)),
        "Dry Hop" => ("add_to_fermentation", None),
        _ => return None,
    })
}

/// Ported from `stageAndStepFor` in `RecipeAdditionMisc.cpp`.
fn misc_stage_and_step(use_of: &str) -> Option<(&'static str, Option<i64>)> {
    Some(match use_of {
        "Mash" => ("add_to_mash", None),
        "Boil" => ("add_to_boil", Some(2)),
        "Primary" => ("add_to_fermentation", Some(1)),
        "Secondary" => ("add_to_fermentation", Some(2)),
        "Bottling" => ("add_to_package", None),
        _ => return None,
    })
}

/// Apply a legacy "use" to an addition that has no stage of its own.
///
/// An explicit stage always wins, and so does an explicit step: the step may have come from the
/// `STEP` extension tag, which says more than the use does.
fn apply_use(record: &mut MappedRecord, property: &str, stage_and_step: StageAndStepFor, fallback: &str) {
    let used = record.legacy.get(property).and_then(Value::as_str).and_then(stage_and_step);
    if !record.columns.contains_key("stage") {
        match used {
            Some((stage, step)) => {
                record.columns.insert("stage".to_owned(), Value::String(stage.to_owned()));
                if let (Some(step), false) = (step, record.columns.contains_key("step")) {
                    record.columns.insert("step".to_owned(), Value::from(step));
                }
            }
            None => {
                record.columns.insert("stage".to_owned(), Value::String(fallback.to_owned()));
            }
        }
    }
}

/// A hop addition, and what BeerXML's `TIME` means for it.
///
/// Ported from the `NamedParameterBundle` constructor of `RecipeAdditionHop`.  `TIME` is read
/// against `USE` even when an explicit stage was given, because a file this application wrote put
/// the time there according to the use.
fn expand_hop_addition(record: &mut MappedRecord) {
    apply_use(record, "RecipeAdditionHop::use", hop_stage_and_step, "add_to_boil");

    let Some(time) = record.legacy.get("RecipeAdditionHop::beerXmlTime_mins").and_then(Value::as_f64) else {
        return;
    };
    let use_of = record.legacy.get("RecipeAdditionHop::use").and_then(Value::as_str).unwrap_or("Boil");
    match use_of {
        // A first wort hop goes in at the start of the ramp-up, so has no time of its own.
        // BeerXML's TIME for it is the length of the boil, which the boil itself records.
        "First Wort" => {}
        // For an aroma or a dry hop, BeerXML's TIME is how long the hops stay in rather than when
        // they go in.  A file we wrote also has the DURATION_MINS extension tag, which says the
        // same thing, so it is not overwritten.
        "Aroma" | "Dry Hop" => {
            record.columns.entry("duration_mins".to_owned()).or_insert_with(|| Value::from(time));
        }
        _ => {
            record.columns.insert("add_at_time_mins".to_owned(), Value::from(time));
        }
    }
}

/// Ported from the `NamedParameterBundle` constructor of `RecipeAdditionMisc`.
fn expand_misc_addition(record: &mut MappedRecord) {
    apply_use(record, "RecipeAdditionMisc::use", misc_stage_and_step, "add_to_boil");
}

/// Ported from the `NamedParameterBundle` constructor of `RecipeAdditionYeast`.
///
/// BeerXML says only whether the yeast goes into the secondary, which is a step number.
fn expand_yeast_addition(record: &mut MappedRecord) {
    default_stage(record, "add_to_fermentation");
    if !record.columns.contains_key("step")
        && let Some(secondary) = record.legacy.get("RecipeAdditionYeast::addToSecondary").and_then(Value::as_bool)
    {
        record.columns.insert("step".to_owned(), Value::from(if secondary { 2 } else { 1 }));
    }
}

/// Pick out the values a path prefix names, with the prefix taken off.
///
/// `Recipe::fermentation/Fermentation::primary/StepBase::startTemp_c` under the prefix
/// `Recipe::fermentation` is `Fermentation::primary/StepBase::startTemp_c`, which is what the
/// C++'s `NamedParameterBundle::getBundle` hands back.
fn bundle(legacy: &BTreeMap<String, Value>, prefix: &str) -> BTreeMap<String, Value> {
    let prefix = format!("{prefix}/");
    legacy.iter().filter_map(|(key, value)| Some((key.strip_prefix(&prefix)?.to_owned(), value.clone()))).collect()
}

/// Build a record of a table out of columns worked out here rather than read from the file.
fn made_up(table: &'static str, columns: Map<String, Value>) -> MappedRecord {
    MappedRecord {
        table,
        columns,
        local_id: None,
        references: BTreeMap::new(),
        folder_path: None,
        legacy: BTreeMap::new(),
    }
}

/// A recipe's boil and fermentation, which BeerXML writes as fields on the recipe itself.
///
/// Ported from `XmlRecipeRecord::normaliseAndStoreChildRecordsInDb`.
fn expand_recipe(owned: &mut Owned) {
    let name = owned.record.columns.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();

    let boil = bundle(&owned.record.legacy, "Recipe::boil");
    if !boil.is_empty() {
        owned.children.push(Child {
            path: String::new(),
            // The recipe points at its boil, so the boil has to be stored first.
            nesting: Nesting::Referenced,
            records: vec![make_boil(&boil, &name, owned)],
        });
    }

    let fermentation = bundle(&owned.record.legacy, "Recipe::fermentation");
    if !fermentation.is_empty()
        && let Some(record) = make_fermentation(&fermentation, &name)
    {
        owned.children.push(Child { path: String::new(), nesting: Nesting::Referenced, records: vec![record] });
    }
}

/// The boil a BeerXML recipe implies: a pre-boil, the boil itself, and a cool-down.
fn make_boil(boil: &BTreeMap<String, Value>, recipe_name: &str, recipe: &Owned) -> Owned {
    let mut columns = Map::new();
    columns.insert("name".to_owned(), Value::String(format!("Boil for {recipe_name}")));
    columns.insert("description".to_owned(), Value::String("Automatically created by BeerXML import".to_owned()));
    if let Some(size) = boil.get("Boil::preBoilSize_l") {
        columns.insert("pre_boil_size_l".to_owned(), size.clone());
    }

    // The generated steps start from where the mash left off and cool to where fermentation
    // begins, so the boil has to be built after the rest of the recipe has been read.
    let surroundings = bt_model::boil::Surroundings {
        recipe_name: recipe_name.to_owned(),
        last_mash_step_temps_c: last_mash_step_temps(recipe),
        first_fermentation_step_start_temp_c: None,
    };
    let mut steps = Vec::new();
    match boil.get("Boil::boilTime_mins").and_then(Value::as_f64) {
        Some(minutes) => bt_model::boil::set_boil_time_mins(&mut steps, &surroundings, minutes),
        None => bt_model::boil::ensure_standard_profile(&mut steps, &surroundings),
    }

    let steps: Vec<Owned> = steps
        .into_iter()
        .map(|step| Owned { record: made_up("boil_step", row_columns(&step)), children: Vec::new() })
        .collect();

    Owned {
        record: made_up("boil", columns),
        children: vec![Child { path: String::new(), nesting: Nesting::Owned, records: steps }],
    }
}

/// The temperatures the last mash step runs between, where the recipe has a mash.
fn last_mash_step_temps(recipe: &Owned) -> Option<(f64, Option<f64>)> {
    let mash = recipe.children.iter().flat_map(|child| &child.records).find(|record| record.record.table == "mash")?;
    let last = mash.children.iter().flat_map(|child| &child.records).last()?;
    let start = last.record.columns.get("step_temp_c").and_then(Value::as_f64)?;
    Some((start, last.record.columns.get("end_temp_c").and_then(Value::as_f64)))
}

/// Turn a generated row back into the columns a record carries.
fn row_columns<T: serde::Serialize>(row: &T) -> Map<String, Value> {
    match serde_json::to_value(row) {
        Ok(Value::Object(mut columns)) => {
            // The database assigns the key; a row that has never been stored has none.
            columns.remove("id");
            columns.retain(|_, value| !value.is_null());
            columns
        }
        _ => Map::new(),
    }
}

/// The fermentation a BeerXML recipe implies, from its primary, secondary and tertiary ages.
///
/// Secondary is ignored without a primary and tertiary without a secondary, which is the
/// assumption `XmlRecipeRecord` makes and says so.
fn make_fermentation(fermentation: &BTreeMap<String, Value>, recipe_name: &str) -> Option<Owned> {
    let mut columns = Map::new();
    columns.insert("name".to_owned(), Value::String(format!("Fermentation for {recipe_name}")));
    columns.insert("description".to_owned(), Value::String("Automatically created by BeerXML import".to_owned()));

    let mut steps = Vec::new();
    for (index, (property, label)) in [
        ("Fermentation::primary", "Primary"),
        ("Fermentation::secondary", "Secondary"),
        ("Fermentation::tertiary", "Tertiary"),
    ]
    .into_iter()
    .enumerate()
    {
        let step = bundle(fermentation, property);
        if step.is_empty() {
            break;
        }
        let mut columns = Map::new();
        columns.insert("name".to_owned(), Value::String(format!("{label} Fermentation Step for {recipe_name}")));
        columns.insert("description".to_owned(), Value::String("Automatically created by BeerXML import".to_owned()));
        columns.insert("step_number".to_owned(), Value::from(index as i64 + 1));
        if let Some(days) = step.get("StepBase::stepTime_days").and_then(Value::as_f64) {
            columns.insert("step_time_mins".to_owned(), Value::from(days * MINUTES_IN_A_DAY));
        }
        if let Some(temperature) = step.get("StepBase::startTemp_c") {
            columns.insert("start_temp_c".to_owned(), temperature.clone());
        }
        steps.push(Owned { record: made_up("fermentation_step", columns), children: Vec::new() });
    }

    // FERMENTATION_STAGES says how many steps there are, which is already apparent from how many
    // there are.  The C++ reads it and ignores it, and so does this.
    if steps.is_empty() {
        return None;
    }
    Some(Owned {
        record: made_up("fermentation", columns),
        children: vec![Child { path: String::new(), nesting: Nesting::Owned, records: steps }],
    })
}

/// Put back into the record what `expand` took out of it.
///
/// A format that has no boil record still has to say how long the boil is, so writing one means
/// going the other way: the boil and the fermentation become fields on the recipe again, and the
/// records themselves are dropped because there is nowhere in the file to put them.
///
/// Only a definition that names property paths needs this, which is BeerXML and nothing else.
pub fn fold(definition: &crate::data::Record, owned: &Owned) -> Option<Owned> {
    if owned.record.table != "recipe" || !definition.fields.iter().any(|field| field.property_path.is_some()) {
        return None;
    }

    let mut folded = owned.clone();
    let mut legacy = BTreeMap::new();

    // The hops have to be dealt with before the boil is taken off the recipe, because what
    // BeerXML says about a hop addition depends on where in the boil it goes.
    fold_hop_additions(&mut folded);

    if let Some(boil) = take_child(&mut folded, "boil") {
        if let Some(size) = boil.record.columns.get("pre_boil_size_l") {
            legacy.insert("Recipe::boil/Boil::preBoilSize_l".to_owned(), size.clone());
        }
        let steps: Vec<bt_model::tables::BoilStepRow> = boil
            .children
            .iter()
            .flat_map(|child| &child.records)
            .filter_map(|step| serde_json::from_value(Value::Object(step.record.columns.clone())).ok())
            .collect();
        legacy
            .insert("Recipe::boil/Boil::boilTime_mins".to_owned(), Value::from(bt_model::boil::boil_time_mins(&steps)));
    }

    if let Some(fermentation) = take_child(&mut folded, "fermentation") {
        let steps: Vec<&MappedRecord> =
            fermentation.children.iter().flat_map(|child| &child.records).map(|step| &step.record).collect();
        legacy.insert("Recipe::fermentation/StepOwnerBase::numSteps".to_owned(), Value::from(steps.len()));
        let named = ["Fermentation::primary", "Fermentation::secondary", "Fermentation::tertiary"];
        for (step, property) in steps.iter().zip(named) {
            if let Some(minutes) = step.columns.get("step_time_mins").and_then(Value::as_f64) {
                legacy.insert(
                    format!("Recipe::fermentation/{property}/StepBase::stepTime_days"),
                    Value::from(minutes / MINUTES_IN_A_DAY),
                );
            }
            if let Some(temperature) = step.columns.get("start_temp_c") {
                legacy.insert(format!("Recipe::fermentation/{property}/StepBase::startTemp_c"), temperature.clone());
            }
        }
    }

    if legacy.is_empty() {
        return None;
    }
    folded.record.legacy = legacy;
    Some(folded)
}

/// Take the records of one table off a parent, returning the first of them.
fn take_child(owned: &mut Owned, table: &str) -> Option<Owned> {
    let position = owned
        .children
        .iter()
        .position(|child| child.records.first().is_some_and(|record| record.record.table == table))?;
    owned.children.remove(position).records.into_iter().next()
}

/// Say what BeerXML says about each hop addition: a use, and a time that means whatever that use
/// makes it mean.
///
/// Ported from `RecipeAdditionHop::use` and `RecipeAdditionHop::beerXmlTime_mins`, which read the
/// recipe's boil -- so this is done while the recipe still has one.
fn fold_hop_additions(recipe: &mut Owned) {
    let boil_steps: Vec<bt_model::tables::BoilStepRow> = recipe
        .children
        .iter()
        .flat_map(|child| &child.records)
        .filter(|record| record.record.table == "boil")
        .flat_map(|boil| &boil.children)
        .flat_map(|child| &child.records)
        .filter_map(|step| serde_json::from_value(Value::Object(step.record.columns.clone())).ok())
        .collect();

    for addition in recipe
        .children
        .iter_mut()
        .flat_map(|child| &mut child.records)
        .filter(|record| record.record.table == "hop_in_recipe")
    {
        let Ok(row) =
            serde_json::from_value::<bt_model::tables::HopInRecipeRow>(Value::Object(addition.record.columns.clone()))
        else {
            continue;
        };
        let used = bt_model::hop_addition::hop_use(&row, &boil_steps);
        addition.record.legacy.insert("RecipeAdditionHop::use".to_owned(), Value::String(used.as_str().to_owned()));
        if let Some(time) = bt_model::hop_addition::beer_xml_time_mins(&row, &boil_steps) {
            addition.record.legacy.insert("RecipeAdditionHop::beerXmlTime_mins".to_owned(), Value::from(time));
        }
    }
}
