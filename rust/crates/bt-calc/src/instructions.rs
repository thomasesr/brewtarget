// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/instructions.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from Recipe::generateInstructions and its helpers in src/model/Recipe.cpp

//! Writing a brew day sheet from a recipe.
//!
//! Ported from `Recipe::generateInstructions`.  The shape is the same: every instruction is
//! collected as a `PreInstruction` carrying where in the brew it belongs, then the lot is
//! sorted once at the end.  That is what lets the helpers run in whatever order suits them
//! rather than in the order a brewer reads them.
//!
//! The C++ renders its amounts through the user's unit preferences.  Stage 1 has no
//! preferences, so these come out metric, which is what `Measurement::displayAmount` gives a
//! metric user.

use bt_model::enums::{MashStepType, RecipeAdditionStage};
use bt_model::recipe::{Addition, IngredientAmount, Recipe};

/// Where in the brew an instruction belongs, before the lot is put in order.
///
/// Ported from the `PreInstruction` struct.
#[derive(Debug, Clone, PartialEq)]
struct PreInstruction {
    stage: RecipeAdditionStage,
    /// The step within the stage, where the instruction belongs to one.
    step: Option<i32>,
    /// Time *remaining*, which is why the sort inverts it.
    interval_mins: f64,
    name: String,
    directions: String,
}

/// One line of the brew day sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Instruction {
    /// Counted from one, in the order they are to be done.
    pub step_number: i32,
    pub name: String,
    pub directions: String,
    /// How long is left of the stage when this is done.
    pub interval_mins: f64,
}

/// How the stages follow one another.
///
/// The C++ gets this from the order the enum is declared in; naming it here means the ordering
/// does not quietly depend on that.
fn stage_order(stage: RecipeAdditionStage) -> u8 {
    match stage {
        RecipeAdditionStage::Mash => 0,
        RecipeAdditionStage::Boil => 1,
        RecipeAdditionStage::Fermentation => 2,
        RecipeAdditionStage::Packaging => 3,
    }
}

/// A volume in litres, as the sheet writes it.
fn litres(value: f64, precision: usize) -> String {
    format!("{value:.precision$} L")
}

/// A temperature in degrees Celsius.
fn celsius(value: f64, precision: usize) -> String {
    format!("{value:.precision$} °C")
}

/// A length of time in minutes.
fn minutes(value: f64) -> String {
    format!("{value:.0} min")
}

/// How much of an ingredient, with its unit.
fn amount_of<Row: IngredientAmount, Ingredient>(addition: &Addition<Row, Ingredient>) -> String {
    match addition.addition.amount() {
        Some(amount) => format!("{:.3} {}", amount.quantity, amount.unit.name()),
        None => "some".to_owned(),
    }
}

/// `<amount> <name>` for each addition, as the C++'s `getNameAndAmounts` helpers give it.
fn names_and_amounts<Row: IngredientAmount, Ingredient>(
    additions: &[Addition<Row, Ingredient>],
    name_of: impl Fn(&Ingredient) -> Option<&str>,
    keep: impl Fn(&Addition<Row, Ingredient>) -> bool,
) -> Vec<String> {
    additions
        .iter()
        .filter(|addition| keep(addition))
        .filter_map(|addition| {
            let name = name_of(addition.ingredient.as_ref()?)?;
            Some(format!("{} {name}", amount_of(addition)))
        })
        .collect()
}

/// Write the brew day sheet for a recipe.
///
/// Ported from `Recipe::generateInstructions`.  Every instruction is collected with the stage
/// and step it belongs to, then sorted once: by stage, then by step, then by time remaining
/// counted *down*, so that an addition with an hour left comes before one with ten minutes.
pub fn generate(recipe: &Recipe) -> Vec<Instruction> {
    let mut found: Vec<PreInstruction> = Vec::new();

    // Mash.
    if !recipe.mash_steps().is_empty() {
        mash_fermentables(recipe, &mut found);
        mash_water(recipe, &mut found);
        mash_steps(recipe, &mut found);
        additions_for(recipe, RecipeAdditionStage::Mash, &mut found);
    }

    first_wort_hops(recipe, &mut found);
    top_off(recipe, &mut found);

    // Boil.
    let boil_time_mins = crate::recipe::boil_time_mins(recipe);
    if !recipe.boil_steps().is_empty() {
        let (first_proper, last_proper) = boil_proper_steps(recipe);
        // No boil, or one of no length, means no "Start boil" and no "Flameout"; additions are
        // then all shown as made at the end.
        let has_boil_proper = boil_time_mins.abs() > f64::EPSILON;
        if has_boil_proper {
            found.push(PreInstruction {
                stage: RecipeAdditionStage::Boil,
                step: first_proper,
                interval_mins: boil_time_mins,
                name: "Start boil".to_owned(),
                directions: format!("Bring the wort to a boil and hold for {}.", minutes(boil_time_mins)),
            });
        }
        boil_fermentables(recipe, boil_time_mins, &mut found);
        additions_for(recipe, RecipeAdditionStage::Boil, &mut found);
        if has_boil_proper {
            found.push(PreInstruction {
                stage: RecipeAdditionStage::Boil,
                step: last_proper,
                interval_mins: 0.0,
                name: "Flameout".to_owned(),
                directions: "Stop boiling the wort.".to_owned(),
            });
        }
    }

    // Fermentation.
    if !recipe.fermentation_steps().is_empty() {
        pitch_yeast(recipe, &mut found);
        additions_for(recipe, RecipeAdditionStage::Fermentation, &mut found);
        ferment(recipe, &mut found);
    }

    // One sort at the end, stable so that instructions pushed in a deliberate order within the
    // same stage, step and time keep it -- "Start boil" before the additions made at its start.
    found.sort_by(|left, right| {
        stage_order(left.stage)
            .cmp(&stage_order(right.stage))
            .then(left.step.cmp(&right.step))
            // Time *remaining*, so more of it comes first.
            .then((-left.interval_mins).total_cmp(&-right.interval_mins))
    });

    found
        .into_iter()
        .enumerate()
        .map(|(index, pre)| Instruction {
            step_number: index as i32 + 1,
            name: pre.name,
            directions: pre.directions,
            interval_mins: pre.interval_mins,
        })
        .collect()
}

/// Which boil steps are the boil proper, numbered from one.
fn boil_proper_steps(recipe: &Recipe) -> (Option<i32>, Option<i32>) {
    use bt_model::boil::{StepPhase, step_phases};
    let phases = step_phases(recipe.boil_steps());
    let proper: Vec<i32> = phases
        .iter()
        .enumerate()
        .filter(|(_, phase)| **phase == StepPhase::BoilProper)
        .map(|(index, _)| index as i32 + 1)
        .collect();
    match (proper.first(), proper.last()) {
        (Some(first), Some(last)) => (Some(*first), Some(*last)),
        // Nothing identifiably the boil proper: fall back to the first and last steps.
        _ => (Some(1), Some(recipe.boil_steps().len() as i32)),
    }
}

fn mash_fermentables(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    let names = names_and_amounts(
        &recipe.fermentables,
        |malt| malt.name.as_deref(),
        |addition| addition.addition.stage().ok().flatten() == Some(RecipeAdditionStage::Mash),
    );
    if names.is_empty() {
        return;
    }
    found.push(PreInstruction {
        stage: RecipeAdditionStage::Mash,
        step: None,
        interval_mins: 0.0,
        name: "Add grains".to_owned(),
        directions: format!("Add {} to the mash tun.", names.join(", ")),
    });
}

fn mash_water(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    let to_heat: Vec<String> = recipe
        .mash_steps()
        .iter()
        .filter(|step| step.mstype().ok().flatten() == Some(MashStepType::Infusion))
        .filter_map(|step| {
            let amount_l = step.amount_l?;
            let temp_c = step.infuse_temp_c.or(step.step_temp_c)?;
            Some(format!("{} of water to {}", litres(amount_l, 1), celsius(temp_c, 1)))
        })
        .collect();
    if to_heat.is_empty() {
        return;
    }
    found.push(PreInstruction {
        stage: RecipeAdditionStage::Mash,
        step: None,
        interval_mins: 0.0,
        name: "Heat water".to_owned(),
        directions: format!("Bring {} for upcoming infusions.", to_heat.join(", ")),
    });
}

fn mash_steps(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    let steps = recipe.mash_steps();
    let mut remaining_mins: f64 = steps.iter().filter_map(|step| step.step_time_mins).sum();

    for (index, step) in steps.iter().enumerate() {
        let kind = step.mstype().ok().flatten();
        let start_temp_c = step.step_temp_c.unwrap_or_default();
        let mut directions = match kind {
            Some(MashStepType::Infusion) => format!(
                "Add {} water at {} to mash to bring it to {}.",
                litres(step.amount_l.unwrap_or_default(), 1),
                celsius(step.infuse_temp_c.unwrap_or(start_temp_c), 1),
                celsius(start_temp_c, 1),
            ),
            Some(MashStepType::Temperature) => format!("Heat mash to {}.", celsius(start_temp_c, 1)),
            Some(MashStepType::Decoction) => format!(
                "Bring {} of the mash to a boil and return to the mash tun to bring it to {}.",
                litres(step.amount_l.unwrap_or_default(), 1),
                celsius(start_temp_c, 1),
            ),
            _ => String::new(),
        };
        let step_time_mins = step.step_time_mins.unwrap_or_default();
        directions.push_str(&format!(" Hold for {}.", minutes(step_time_mins)));

        let kind_name = kind.map(|kind| kind.display_name()).unwrap_or("Mash");
        found.push(PreInstruction {
            stage: RecipeAdditionStage::Mash,
            // The step's position, which is what `StepOwnerBase::sequenceNumber` gives the
            // C++ -- not the `step_number` column, which is often not set at all.  Getting
            // this wrong leaves every mash instruction tied, and the tiebreak then puts "Add
            // grains" after the step that uses them.
            step: Some(index as i32 + 1),
            interval_mins: remaining_mins,
            name: format!("{kind_name} - {}", step.name.as_deref().unwrap_or_default()),
            directions,
        });
        remaining_mins -= step_time_mins;
    }
}

/// What to say about an addition made at a given stage.
///
/// Ported from `instructionsForRecipeAddition`.  The wording depends on what is being added
/// and where: a hop in the boil proper goes "into boil", one after it is "steeped in wort",
/// and one at fermentation goes "into fermentor".
fn additions_for(recipe: &Recipe, stage: RecipeAdditionStage, found: &mut Vec<PreInstruction>) {
    let stage_length_mins = match stage {
        RecipeAdditionStage::Mash => recipe.mash_steps().iter().filter_map(|step| step.step_time_mins).sum(),
        RecipeAdditionStage::Boil => crate::recipe::boil_time_mins(recipe),
        RecipeAdditionStage::Fermentation => {
            recipe.fermentation_steps().iter().filter_map(|step| step.step_time_mins).sum()
        }
        RecipeAdditionStage::Packaging => 0.0,
    };

    // Hops first, then miscs, which is the order the C++ asks for them in.
    for addition in &recipe.hops {
        if addition.addition.stage().ok().flatten() != Some(stage) {
            continue;
        }
        let Some(hop) = addition.ingredient.as_ref() else { continue };
        let what = format!("{} {}", amount_of(addition), hop.name.as_deref().unwrap_or_default());
        let step = addition.addition.step;
        let duration_mins = addition_duration_mins(
            recipe,
            stage,
            &addition.addition.duration_mins,
            addition.addition.add_at_time_mins,
            stage_length_mins,
        );
        let wording = match stage {
            RecipeAdditionStage::Mash => format!("Put {what} into mash"),
            RecipeAdditionStage::Boil => {
                use bt_model::hop_addition::{HopUse, hop_use};
                match hop_use(&addition.addition, recipe.boil_steps()) {
                    HopUse::FirstWort => format!("Put {what} into first wort"),
                    HopUse::Aroma => format!("Steep {what} in wort"),
                    _ => format!("Put {what} into boil"),
                }
            }
            RecipeAdditionStage::Fermentation => format!("Put {what} into fermentor"),
            RecipeAdditionStage::Packaging => format!("Put {what} into the package"),
        };
        found.push(PreInstruction {
            stage,
            step,
            interval_mins: duration_mins,
            name: format!("Add {}", hop.name.as_deref().unwrap_or_default()),
            directions: format!("{wording}{}.", for_how_long(duration_mins)),
        });
    }

    for addition in &recipe.miscs {
        if addition.addition.stage().ok().flatten() != Some(stage) {
            continue;
        }
        let Some(misc) = addition.ingredient.as_ref() else { continue };
        let what = format!("{} {}", amount_of(addition), misc.name.as_deref().unwrap_or_default());
        let step = addition.addition.step;
        let duration_mins = addition_duration_mins(
            recipe,
            stage,
            &addition.addition.duration_mins,
            addition.addition.add_at_time_mins,
            stage_length_mins,
        );
        let wording = match stage {
            RecipeAdditionStage::Mash => format!("Put {what} into mash"),
            RecipeAdditionStage::Boil => format!("Put {what} into boil"),
            // The C++ distinguishes primary from secondary by the step number.
            RecipeAdditionStage::Fermentation if step == Some(1) => format!("Put {what} into primary"),
            RecipeAdditionStage::Fermentation => format!("Put {what} into secondary"),
            RecipeAdditionStage::Packaging => format!("Put {what} into the package"),
        };
        found.push(PreInstruction {
            stage,
            step,
            interval_mins: duration_mins,
            name: format!("Add {}", misc.name.as_deref().unwrap_or_default()),
            directions: format!("{wording}{}.", for_how_long(duration_mins)),
        });
    }
}

/// How long an addition is in for.
///
/// A recorded duration wins.  Otherwise, for the boil, it is how long the addition spends in
/// it, worked out from when it goes in; elsewhere it is what is left of the stage.
fn addition_duration_mins(
    recipe: &Recipe,
    stage: RecipeAdditionStage,
    duration_mins: &Option<f64>,
    add_at_time_mins: Option<f64>,
    stage_length_mins: f64,
) -> f64 {
    if let Some(duration) = duration_mins {
        return *duration;
    }
    if stage == RecipeAdditionStage::Boil {
        // For the boil, the time is counted back from the end, so it is already how long the
        // addition is in for.
        let _ = recipe;
        return add_at_time_mins.unwrap_or(stage_length_mins);
    }
    (stage_length_mins - add_at_time_mins.unwrap_or(0.0)).max(0.0)
}

/// " for 60 min", or nothing at all where there is no time to give.
fn for_how_long(duration_mins: f64) -> String {
    if duration_mins > 0.0 { format!(" for {}", minutes(duration_mins)) } else { String::new() }
}

fn first_wort_hops(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    use bt_model::hop_addition::{HopUse, hop_use};
    let names = names_and_amounts(
        &recipe.hops,
        |hop| hop.name.as_deref(),
        |addition| hop_use(&addition.addition, recipe.boil_steps()) == HopUse::FirstWort,
    );
    if names.is_empty() {
        return;
    }
    found.push(PreInstruction {
        stage: RecipeAdditionStage::Boil,
        // First wort hops go in at the start of the boil.
        step: Some(1),
        interval_mins: 0.0,
        name: "First wort hopping".to_owned(),
        directions: format!("Do first wort hopping with {}.", names.join(", ")),
    });
}

fn top_off(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    let Some(equipment) = recipe.equipment.as_ref() else { return };
    let figures = crate::recipe::calculate_recipe(recipe, Default::default(), Default::default());
    let mut wort_in_boil_l = figures.wort_from_mash_l - crate::recipe::lautering_deadspace_loss_l(equipment);
    let mut directions = format!("You should now have {} wort.", litres(wort_in_boil_l, 1));

    let top_up_l = equipment.top_up_kettle.unwrap_or(0.0);
    if top_up_l == 0.0 {
        return;
    }
    wort_in_boil_l += top_up_l;
    directions.push_str(&format!(
        " Add {} water to the kettle, bringing pre-boil volume to {}.",
        litres(top_up_l, 1),
        litres(wort_in_boil_l, 1),
    ));
    found.push(PreInstruction {
        stage: RecipeAdditionStage::Boil,
        step: Some(1),
        interval_mins: 0.0,
        name: "Pre-boil".to_owned(),
        directions,
    });
}

fn boil_fermentables(recipe: &Recipe, boil_time_mins: f64, found: &mut Vec<PreInstruction>) {
    use bt_model::enums::FermentableType;
    let names = names_and_amounts(
        &recipe.fermentables,
        |malt| malt.name.as_deref(),
        |addition| {
            addition.addition.stage().ok().flatten() == Some(RecipeAdditionStage::Boil)
            // Extracts get an instruction of their own, a minute before the end.
            && !matches!(
                addition.ingredient.as_ref().and_then(|malt| malt.ftype().ok().flatten()),
                Some(FermentableType::Extract | FermentableType::DryExtract)
            )
        },
    );
    if !names.is_empty() {
        found.push(PreInstruction {
            stage: RecipeAdditionStage::Boil,
            step: None,
            interval_mins: boil_time_mins,
            name: "Boil fermentables".to_owned(),
            directions: format!("Boil or steep {}.", names.join(", ")),
        });
    }

    // Extracts go in near the end, so that they are not boiled for the whole hour.
    let extracts = names_and_amounts(
        &recipe.fermentables,
        |malt| malt.name.as_deref(),
        |addition| {
            addition.addition.stage().ok().flatten() == Some(RecipeAdditionStage::Boil)
                && matches!(
                    addition.ingredient.as_ref().and_then(|malt| malt.ftype().ok().flatten()),
                    Some(FermentableType::Extract | FermentableType::DryExtract)
                )
        },
    );
    if !extracts.is_empty() {
        found.push(PreInstruction {
            stage: RecipeAdditionStage::Boil,
            step: None,
            // One minute before the end, and never negative when there is no boil to speak of.
            interval_mins: (boil_time_mins - 1.0).max(0.0),
            name: "Add extracts to boil".to_owned(),
            directions: format!("Add {} to the boil.", extracts.join(", ")),
        });
    }
}

fn pitch_yeast(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    let mut directions = "Cool wort and pitch ".to_owned();
    for addition in &recipe.yeasts {
        if addition.addition.step != Some(1) {
            continue;
        }
        let Some(yeast) = addition.ingredient.as_ref() else { continue };
        let kind = yeast.ytype().ok().flatten().map(|kind| kind.display_name()).unwrap_or_default();
        directions.push_str(&format!("{} {kind} yeast, ", yeast.name.as_deref().unwrap_or_default()));
    }
    directions.push_str("to the primary.");
    found.push(PreInstruction {
        stage: RecipeAdditionStage::Fermentation,
        step: Some(1),
        interval_mins: 0.0,
        name: "Pitch yeast".to_owned(),
        directions,
    });
}

fn ferment(recipe: &Recipe, found: &mut Vec<PreInstruction>) {
    let figures = crate::recipe::calculate_recipe(recipe, Default::default(), Default::default());
    found.push(PreInstruction {
        stage: RecipeAdditionStage::Fermentation,
        step: Some(1),
        interval_mins: 0.0,
        name: "Ferment".to_owned(),
        directions: format!("Let ferment until FG is {:.3}.", figures.fg),
    });
    if recipe.fermentation_steps().len() > 1 {
        found.push(PreInstruction {
            stage: RecipeAdditionStage::Fermentation,
            step: Some(2),
            interval_mins: 0.0,
            name: "Transfer to secondary".to_owned(),
            directions: "Transfer beer to secondary.".to_owned(),
        });
    }
}
