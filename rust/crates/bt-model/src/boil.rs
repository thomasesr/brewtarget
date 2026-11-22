// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-model/src/boil.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/model/Boil.cpp

//! The boil, and the shape a boil is expected to have.
//!
//! A boil is a list of steps, and only the middle of that list is actually boiling: the wort has
//! to be brought up to temperature and comes back down afterwards.  Which steps count as the boil
//! proper is worked out from their temperatures rather than their positions, so a profile with a
//! whirlpool or a hop stand in it is read correctly.

use crate::tables::BoilStepRow;

/// `Boil::minimumBoilTemperature_c`: at or above this the wort counts as boiling.
pub const MINIMUM_BOIL_TEMPERATURE_C: f64 = 81.0;

/// Which part of the boil a step belongs to.
///
/// Ported from `Boil::StepPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPhase {
    PreBoil,
    BoilProper,
    PostBoil,
}

/// A step with no temperature recorded is taken to be at the boil.
///
/// `.beer` says an unset boil step temperature means "at boiling", and the C++ uses an infinity
/// sentinel to say so, which is what this is.  It is only ever compared, never used as a figure.
fn start_temp_for_comparison(step: &BoilStepRow) -> f64 {
    step.start_temp_c.unwrap_or(f64::INFINITY)
}

fn end_temp_for_comparison(step: &BoilStepRow) -> f64 {
    step.end_temp_c.unwrap_or(f64::INFINITY)
}

/// Whether a step is already at the boil when it starts.  `Boil::startsAtBoil`.
pub fn starts_at_boil(step: &BoilStepRow) -> bool {
    start_temp_for_comparison(step) >= MINIMUM_BOIL_TEMPERATURE_C
}

/// Whether a step is still at the boil when it ends.  `Boil::endsAtBoil`.
pub fn ends_at_boil(step: &BoilStepRow) -> bool {
    end_temp_for_comparison(step) >= MINIMUM_BOIL_TEMPERATURE_C
}

/// Whether a step stays at the boil throughout.
///
/// `Boil::holdsAtBoil`: hot at both ends, and not cooling as it goes.
pub fn holds_at_boil(step: &BoilStepRow) -> bool {
    starts_at_boil(step) && ends_at_boil(step) && end_temp_for_comparison(step) >= start_temp_for_comparison(step)
}

/// Work out which phase each step belongs to.
///
/// Ported from `Boil::stepPhases`.  The phases only go forwards: once the wort has come off the
/// boil it does not go back on, so a later hot step -- a hop stand held at 92 °C -- is still
/// post-boil.
pub fn step_phases(steps: &[BoilStepRow]) -> Vec<StepPhase> {
    let mut phases = Vec::with_capacity(steps.len());
    let mut current = StepPhase::PreBoil;
    let mut previous: Option<&BoilStepRow> = None;

    for step in steps {
        match current {
            StepPhase::PreBoil => {
                if holds_at_boil(step) {
                    current = StepPhase::BoilProper;
                }
            }
            StepPhase::BoilProper => {
                // The boil proper ends as soon as the wort starts to cool, either during a step
                // or between one step and the next.
                if let Some(previous) = previous
                    && (!holds_at_boil(step) || start_temp_for_comparison(step) < end_temp_for_comparison(previous))
                {
                    current = StepPhase::PostBoil;
                }
            }
            StepPhase::PostBoil => {}
        }
        phases.push(current);
        previous = Some(step);
    }
    phases
}

/// How long the wort is actually at the boil.  Ported from `Boil::boilTime_mins`.
pub fn boil_time_mins(steps: &[BoilStepRow]) -> f64 {
    step_phases(steps)
        .into_iter()
        .zip(steps)
        .filter(|(phase, _)| *phase == StepPhase::BoilProper)
        .filter_map(|(_, step)| step.step_time_mins)
        .sum()
}

/// How long the wort takes to cool, if the boil ends by cooling.
///
/// Ported from `Boil::coolTime_mins`, including the note that an unset start temperature must not
/// be read as below boiling.
pub fn cool_time_mins(steps: &[BoilStepRow]) -> Option<f64> {
    let last = steps.last()?;
    if starts_at_boil(last) && !ends_at_boil(last) { last.step_time_mins } else { None }
}

/// What a boil needs to know about the recipe around it to fill in a standard profile.
///
/// The C++ looks these up from the `Recipe` that owns the boil; here they are handed in, because
/// a boil being created while a file is read is not yet attached to anything.
#[derive(Debug, Clone, Default)]
pub struct Surroundings {
    /// The recipe's name, which goes into the names of the steps that get created.
    pub recipe_name: String,
    /// The start and end temperature of the last mash step, which is where the pre-boil starts
    /// from.
    pub last_mash_step_temps_c: Option<(f64, Option<f64>)>,
    /// The temperature fermentation starts at, which is where the post-boil cools down to.
    pub first_fermentation_step_start_temp_c: Option<f64>,
}

impl Surroundings {
    /// The name to put in a generated step, as `NamedEntity::strippedName` gives it: a recipe
    /// called "Best Bitter (2)" is the same recipe as "Best Bitter" for this purpose.
    fn recipe(&self) -> &str {
        let trimmed = self.recipe_name.trim_end();
        let Some(opened) = trimmed.strip_suffix(')') else { return &self.recipe_name };
        let digits = opened.trim_end_matches(|character: char| character.is_ascii_digit());
        if digits.len() == opened.len() {
            return &self.recipe_name;
        }
        match digits.trim_end_matches(['(', ' ']).strip_suffix('(') {
            Some(before) => before.trim_end(),
            None => &self.recipe_name,
        }
    }
}

/// A step the reader made up rather than read.
fn generated_step(name: String, description: String, start_temp_c: f64, end_temp_c: f64) -> BoilStepRow {
    BoilStepRow {
        name: Some(name),
        description: Some(description),
        start_temp_c: Some(start_temp_c),
        end_temp_c: Some(end_temp_c),
        ..BoilStepRow::default()
    }
}

/// Give a boil the three steps a boil is assumed to have: ramp up, boil, cool down.
///
/// Ported from `Boil::ensureStandardProfile`.  This is what turns BeerXML's single `BOIL_TIME`
/// into something with a shape.  Steps are classified by their temperatures rather than their
/// positions, and the list is re-read after each insertion, so a boil that already has a cooling
/// step gets a pre-boil step added without also gaining a second cooling one.
pub fn ensure_standard_profile(steps: &mut Vec<BoilStepRow>, surroundings: &Surroundings) {
    let recipe = surroundings.recipe().to_owned();

    if steps.first().is_none_or(starts_at_boil) {
        // A ramp-up step, starting from wherever the mash left off.
        let mut starting_temp_c = MINIMUM_BOIL_TEMPERATURE_C - 1.0;
        if let Some((start, end)) = surroundings.last_mash_step_temps_c {
            starting_temp_c = start.min(end.unwrap_or(starting_temp_c));
        }
        steps.insert(
            0,
            generated_step(
                format!("Pre-boil for {recipe}"),
                format!("Automatically-generated pre-boil step for {recipe}"),
                starting_temp_c,
                100.0,
            ),
        );
    }

    if !step_phases(steps).contains(&StepPhase::BoilProper) {
        // The boil proper goes before the first step that cools down from the boil, or at the end
        // if there is not one.  A step that cools from the boil may still end above the boiling
        // threshold -- 100 °C down to 92 °C for a hot whirlpool.
        let position =
            steps.iter().position(|step| starts_at_boil(step) && !holds_at_boil(step)).unwrap_or(steps.len());
        steps.insert(
            position,
            generated_step(
                format!("Main boil for {recipe}"),
                format!("Automatically-generated boil proper step for {recipe}"),
                100.0,
                100.0,
            ),
        );
    }

    // A boil that does not end post-boil needs a cooling step.  Checking that the last step ends
    // at boiling is not enough: a hop stand held at 92 °C after a cooling step is post-boil.
    if step_phases(steps).last() != Some(&StepPhase::PostBoil) {
        let ending_temp_c = surroundings.first_fermentation_step_start_temp_c.unwrap_or(30.0);
        // Start from wherever the boil proper ended, which at altitude is below 100 °C.
        let starting_temp_c = steps.last().and_then(|step| step.end_temp_c).unwrap_or(100.0);
        steps.push(generated_step(
            format!("Post-boil for {recipe}"),
            format!("Automatically-generated post-boil step for {recipe}"),
            starting_temp_c,
            ending_temp_c,
        ));
    }

    renumber(steps);
}

/// Number the steps from one, which is what `StepOwnerBase::insert` does as it goes.
fn renumber(steps: &mut [BoilStepRow]) {
    for (index, step) in steps.iter_mut().enumerate() {
        step.step_number = Some(index as i64 + 1);
    }
}

/// Say how long the boil lasts, which only has meaning once the boil has a shape.
///
/// Ported from `Boil::setBoilTime_mins`: the time goes on the first step of the boil proper, and
/// the profile is filled in first so that there is one.
pub fn set_boil_time_mins(steps: &mut Vec<BoilStepRow>, surroundings: &Surroundings, boil_time_mins: f64) {
    ensure_standard_profile(steps, surroundings);
    let phases = step_phases(steps);
    if let Some(index) = phases.iter().position(|phase| *phase == StepPhase::BoilProper) {
        steps[index].step_time_mins = Some(boil_time_mins);
    }
}
