// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-model/src/hop_addition.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/model/RecipeAdditionHop.cpp

//! What a hop addition amounts to in BeerXML's terms.
//!
//! BeerXML has a single `USE` -- mash, first wort, boil, aroma, dry hop -- where the database has
//! a stage and a step within it.  Going from a use to a stage is a lookup; going back is not,
//! because what distinguishes a first wort hop from a boil hop is *when in the boil* it goes in,
//! which means looking at the boil's steps and their temperatures.

use crate::boil::{StepPhase, step_phases};
use crate::generated::enums::RecipeAdditionStage;
use crate::tables::{BoilStepRow, HopInRecipeRow};

/// BeerXML's `USE`.  Ported from `RecipeAdditionHop::Use`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopUse {
    Mash,
    FirstWort,
    Boil,
    Aroma,
    DryHop,
}

impl HopUse {
    /// The spelling `RecipeAdditionHop::useStringMapping` gives.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mash => "Mash",
            Self::FirstWort => "First Wort",
            Self::Boil => "Boil",
            Self::Aroma => "Aroma",
            Self::DryHop => "Dry Hop",
        }
    }
}

/// Whether an addition goes in before the boil proper.
///
/// Ported from `RecipeAdditionHop::isFirstWort`.  `.beer` says a first wort addition is one made
/// in a pre-boil step, which is identified by its temperature and position rather than by its
/// number -- the pre-boil step is usually, but not necessarily, step one.
///
/// A boil in which nothing is identifiably the boil proper cannot be made sense of this way, so
/// nothing in it is treated as first wort.
pub fn is_first_wort(addition: &HopInRecipeRow, boil_steps: &[BoilStepRow]) -> bool {
    if addition.stage().ok().flatten() != Some(RecipeAdditionStage::Boil) {
        return false;
    }
    let Some(step) = addition.step else { return false };
    let Ok(index) = usize::try_from(step - 1) else { return false };
    if index >= boil_steps.len() {
        return false;
    }
    let phases = step_phases(boil_steps);
    if !phases.contains(&StepPhase::BoilProper) {
        return false;
    }
    phases[index] == StepPhase::PreBoil
}

/// Whether an addition goes in after the boil proper -- flameout, whirlpool, a hop stand.
///
/// Ported from `RecipeAdditionHop::isAroma`.  A post-boil step may still be well above boiling,
/// which is why the phase decides and not the temperature on its own.
pub fn is_aroma(addition: &HopInRecipeRow, boil_steps: &[BoilStepRow]) -> bool {
    if addition.stage().ok().flatten() != Some(RecipeAdditionStage::Boil) {
        return false;
    }
    // An aroma addition is after the first step of the boil.
    let Some(step) = addition.step.filter(|step| *step != 1) else { return false };
    let Ok(index) = usize::try_from(step - 1) else { return false };
    if index >= boil_steps.len() {
        // A step number beyond the end of the boil is a broken record rather than an aroma hop.
        return false;
    }
    step_phases(boil_steps)[index] == StepPhase::PostBoil
}

/// Which of BeerXML's uses this addition is.  Ported from `RecipeAdditionHop::use`.
pub fn hop_use(addition: &HopInRecipeRow, boil_steps: &[BoilStepRow]) -> HopUse {
    match addition.stage().ok().flatten() {
        Some(RecipeAdditionStage::Mash) => HopUse::Mash,
        Some(RecipeAdditionStage::Boil) => {
            if is_first_wort(addition, boil_steps) {
                HopUse::FirstWort
            } else if is_aroma(addition, boil_steps) {
                HopUse::Aroma
            } else {
                HopUse::Boil
            }
        }
        // Everything after the boil is a dry hop as far as BeerXML is concerned.
        _ => HopUse::DryHop,
    }
}

/// `Boil::default_boilTime_mins`, used when a first wort addition has no boil to ask.
pub const DEFAULT_BOIL_TIME_MINS: f64 = 60.0;

/// What BeerXML's `TIME` should say for this addition.
///
/// Ported from `RecipeAdditionHop::beerXmlTime_mins`.  The field means a different thing for each
/// use: when the hops go in for a boil addition, how long they stay in for an aroma or dry hop
/// addition, and the length of the boil for a first wort one.
pub fn beer_xml_time_mins(addition: &HopInRecipeRow, boil_steps: &[BoilStepRow]) -> Option<f64> {
    match hop_use(addition, boil_steps) {
        HopUse::FirstWort => {
            let boil_time = crate::boil::boil_time_mins(boil_steps);
            Some(if boil_steps.is_empty() { DEFAULT_BOIL_TIME_MINS } else { boil_time })
        }
        // Data from older versions may have the steeping or dry hop time in the "add at" time.
        HopUse::Aroma | HopUse::DryHop => addition.duration_mins.or(addition.add_at_time_mins),
        HopUse::Mash | HopUse::Boil => addition.add_at_time_mins,
    }
}
