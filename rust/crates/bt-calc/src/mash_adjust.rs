// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/mash_adjust.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.
//
// Ported from src/MashAutoAdjuster.cpp

//! Working out a mash schedule: strike water, infusions, sparge.
//!
//! A brewer says what temperatures they want to rest at and for how long; this works out how
//! much water to add and how hot it has to be to get there, given the grain, the tun and what
//! is already in it.  It is the arithmetic behind the Qt application's "auto-adjust" on the
//! mash editor, and behind the strike water tool.
//!
//! Ported from `src/MashAutoAdjuster.cpp`.  The heat model is a mixing calculation repeated
//! down the steps: everything in the tun has a heat capacity and a temperature, water is added
//! at some temperature, and the result is whatever the two settle at.

use bt_model::enums::MashStepType;

use crate::data::constant;

/// `PhysicalConstants::waterSpecificHeat_calGC`, in kcal per kg per °C.
fn water_specific_heat() -> f64 {
    constant("waterSpecificHeat_calGC")
}

/// `PhysicalConstants::grainSpecificHeat_calGC`.
fn grain_specific_heat() -> f64 {
    constant("grainSpecificHeat_calGC")
}

/// Close enough to zero to be zero, as `qFuzzyIsNull` has it.
fn is_nothing(value: f64) -> bool {
    value.abs() < 1e-12
}

/// What is in the tun: each part with its heat capacity and its temperature.
///
/// Ported from the `Contents` helper.  Grain, the tun itself and whatever water is already
/// there are all just parts; the arithmetic does not care which is which.
#[derive(Debug, Clone, Default)]
pub struct Contents {
    pub parts: Vec<Part>,
}

/// One thing in the tun.
#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub heat_capacity_kcal_c: f64,
    pub temp_c: f64,
}

impl Contents {
    pub fn heat_capacity_kcal_c(&self) -> f64 {
        self.parts.iter().map(|part| part.heat_capacity_kcal_c).sum()
    }

    /// The heat needed to bring everything to `temp_c`, negative if it needs cooling.
    pub fn heat_needed_kcal(&self, temp_c: f64) -> f64 {
        self.parts.iter().map(|part| part.heat_capacity_kcal_c * (temp_c - part.temp_c)).sum()
    }

    /// Where the temperature settles after adding `amount_l` of water at `water_temp_c`.
    pub fn final_temp_c(&self, amount_l: f64, water_temp_c: f64) -> f64 {
        let specific_heat = water_specific_heat();
        let mut heat_capacity = amount_l * specific_heat;
        let mut energy = amount_l * specific_heat * water_temp_c;
        for part in &self.parts {
            heat_capacity += part.heat_capacity_kcal_c;
            energy += part.heat_capacity_kcal_c * part.temp_c;
        }
        if heat_capacity > 0.0 { energy / heat_capacity } else { water_temp_c }
    }

    /// How hot the water has to be to reach `temp_c` by adding `amount_l` of it.
    ///
    /// The caller has to have checked that `amount_l` is more than nothing.
    pub fn water_temp_for(&self, temp_c: f64, amount_l: f64) -> f64 {
        temp_c + self.heat_needed_kcal(temp_c) / (amount_l * water_specific_heat())
    }

    /// How much water at `water_temp_c` it takes to reach `temp_c`.
    ///
    /// `None` when it cannot be done at all -- water no hotter than the target cannot heat
    /// the mash, however much of it there is.
    pub fn amount_for(&self, temp_c: f64, water_temp_c: f64) -> Option<f64> {
        let heat_needed_kcal = self.heat_needed_kcal(temp_c);
        let water_temp_difference_c = water_temp_c - temp_c;
        if is_nothing(heat_needed_kcal) {
            return Some(0.0);
        }
        if is_nothing(water_temp_difference_c) {
            return None;
        }
        let amount_l = heat_needed_kcal / (water_specific_heat() * water_temp_difference_c);
        if amount_l < 0.0 { None } else { Some(amount_l) }
    }
}

/// What the mash is made of, before any water goes in.
///
/// Ported from `MashAutoAdjuster::Inputs`, less the parts only the full scheduler needs.
#[derive(Debug, Clone, Copy)]
pub struct Mashing {
    pub grain_kg: f64,
    /// The temperature the grain starts at, which is also taken to be the temperature of the
    /// coldest water available -- it is the temperature of the room.
    pub grain_temp_c: f64,
    /// The heat the tun itself soaks up, which is why a cold tun needs hotter strike water.
    pub tun_heat_capacity_kcal_c: f64,
    pub tun_temp_c: f64,
    pub boiling_point_c: f64,
}

impl Default for Mashing {
    fn default() -> Self {
        Self {
            grain_kg: 0.0,
            grain_temp_c: 20.0,
            tun_heat_capacity_kcal_c: 0.0,
            tun_temp_c: 20.0,
            boiling_point_c: 100.0,
        }
    }
}

impl Mashing {
    /// Everything in the tun before the first infusion: the grain and the tun.
    pub fn before_mashing_in(&self) -> Contents {
        Contents {
            parts: vec![
                Part { heat_capacity_kcal_c: self.grain_kg * grain_specific_heat(), temp_c: self.grain_temp_c },
                Part { heat_capacity_kcal_c: self.tun_heat_capacity_kcal_c, temp_c: self.tun_temp_c },
            ],
        }
    }

    /// Everything in the tun once the mash is under way, which settles to one temperature.
    ///
    /// The grain, the tun and the water already in it are no longer at different temperatures,
    /// so they are one part rather than three.
    pub fn during_mash(&self, water_l: f64, temp_c: f64) -> Contents {
        Contents {
            parts: vec![Part {
                heat_capacity_kcal_c: self.grain_kg * grain_specific_heat()
                    + water_l * water_specific_heat()
                    + self.tun_heat_capacity_kcal_c,
                temp_c,
            }],
        }
    }

    /// How hot the strike water has to be to mash in at `target_temp_c`.
    ///
    /// This is what the strike water tool asks and the first step of every infusion mash
    /// needs.  `None` when no water is being added, since then there is nothing to be hot.
    pub fn strike_water_temp_c(&self, target_temp_c: f64, water_l: f64) -> Option<f64> {
        if water_l <= 0.0 {
            return None;
        }
        Some(self.before_mashing_in().water_temp_for(target_temp_c, water_l))
    }

    /// How much water at `water_temp_c` it takes to mash in at `target_temp_c`.
    pub fn strike_water_amount_l(&self, target_temp_c: f64, water_temp_c: f64) -> Option<f64> {
        self.before_mashing_in().amount_for(target_temp_c, water_temp_c)
    }

    /// Whether a temperature is one water can be at: above freezing, below boiling.
    ///
    /// `temperatureMakesSense` in the C++, which uses it to reject a mash that cannot happen
    /// before trying to work one out.
    pub fn temperature_makes_sense(&self, temp_c: f64) -> bool {
        0.0 < temp_c && temp_c < self.boiling_point_c
    }
}

/// How the sparge is done, if there is one.
///
/// Ported from `MashAutoAdjuster::SpargeMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpargeMethod {
    /// Water trickles through the bed continuously, so there is no mixing to balance.
    Fly,
    /// The bed is drained and refilled, once or several times.
    Batch,
    /// No sparge at all: the mash water has to add up to everything the boil needs.
    None,
}

/// One step of a mash, as the adjuster sees it.
///
/// Ported from `MashAutoAdjuster::Step`.  `amount_fixed` and `infuse_temp_fixed` are the pins
/// the brewer puts on a value to say "this one I have decided; work the rest around it".
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub name: String,
    pub step_type: MashStepType,
    pub start_temp_c: f64,
    pub end_temp_c: Option<f64>,
    pub step_time_mins: f64,
    pub amount_l: f64,
    pub infuse_temp_c: Option<f64>,
    pub amount_fixed: bool,
    pub infuse_temp_fixed: bool,
    /// Litres of water per kilogram of grain, counting everything added so far.
    pub liquor_to_grist_ratio_l_kg: Option<f64>,
    /// How big the mash is at this step.  An output.
    pub mash_volume_l: Option<f64>,
}

impl Step {
    pub fn new(name: &str, step_type: MashStepType, start_temp_c: f64) -> Self {
        Self {
            name: name.to_owned(),
            step_type,
            start_temp_c,
            end_temp_c: None,
            step_time_mins: 0.0,
            amount_l: 0.0,
            infuse_temp_c: None,
            amount_fixed: false,
            infuse_temp_fixed: false,
            liquor_to_grist_ratio_l_kg: None,
            mash_volume_l: None,
        }
    }

    /// Whether water is added at this step.
    pub fn is_infusion(&self) -> bool {
        self.step_type == MashStepType::Infusion
    }

    /// Whether this is a sparge rather than part of the mash proper.
    pub fn is_sparge(&self) -> bool {
        matches!(self.step_type, MashStepType::FlySparge | MashStepType::BatchSparge)
    }

    /// The temperature the mash is at when this step finishes.
    pub fn final_temp_c(&self) -> f64 {
        self.end_temp_c.unwrap_or(self.start_temp_c)
    }
}

/// How hard the adjuster should try, and what to assume where the mash does not say.
///
/// Ported from `MashAutoAdjuster::Options`.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Litres of water per kilogram of grain to aim for.  `None` uses the step's own ratio,
    /// then the thickness already there, then [`DEFAULT_THICKNESS_L_KG`].
    pub thickness_l_kg: Option<f64>,
    /// How much the grain bed cools while it drains, before the sparge water meets it.
    pub sparge_heat_loss_c: f64,
    pub sparge_temp_c: f64,
    /// Report problems without changing anything.
    pub check_only: bool,
}

/// `MashAutoAdjuster::defaultThickness_LKg`.
pub const DEFAULT_THICKNESS_L_KG: f64 = 2.6;
/// `MashAutoAdjuster::defaultSpargeTemp_c`.
pub const DEFAULT_SPARGE_TEMP_C: f64 = 76.0;
/// How far a worked-out temperature may be from the one asked for before it is worth saying.
const TEMPERATURE_TOLERANCE_C: f64 = 0.5;
/// ...and the same for volumes.
const VOLUME_TOLERANCE_L: f64 = 0.1;
/// A temperature this far outside what is possible is rounding rather than a mistake, and is
/// brought back into range instead of refused.
const TEMPERATURE_RANGE_TOLERANCE_C: f64 = 0.1;

impl Default for Options {
    fn default() -> Self {
        Self { thickness_l_kg: None, sparge_heat_loss_c: 2.0, sparge_temp_c: DEFAULT_SPARGE_TEMP_C, check_only: false }
    }
}

/// How much a problem matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The schedule cannot be worked out at all.
    Error,
    /// It can, but it will not do quite what was asked.
    Warning,
}

/// What a problem is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The mash as a whole.
    Mash,
    /// One property of the mash.
    MashProperty(&'static str),
    /// One step, counted from zero, and optionally one of its properties.
    Step(usize, Option<&'static str>),
}

/// Something the brewer should be told.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub severity: Severity,
    pub scope: Scope,
    pub message: String,
}

/// What the adjuster made of a mash.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub steps: Vec<Step>,
    pub problems: Vec<Problem>,
    /// The largest the mash gets, for comparing against the tun.
    pub max_mash_volume_l: Option<f64>,
    /// How much wort comes off, where the caller said what it was aiming for.
    pub collected_wort_l: Option<f64>,
}

impl Outcome {
    /// Whether a schedule came out of this at all.
    pub fn ok(&self) -> bool {
        !self.problems.iter().any(|problem| problem.severity == Severity::Error)
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Problem> {
        self.problems.iter().filter(|problem| problem.severity == Severity::Warning)
    }

    fn error(&mut self, scope: Scope, message: String) {
        self.problems.push(Problem { severity: Severity::Error, scope, message });
    }

    fn warn(&mut self, scope: Scope, message: String) {
        self.problems.push(Problem { severity: Severity::Warning, scope, message });
    }
}

/// Give a mash the sparge steps a method calls for, replacing whatever it had.
///
/// Ported from `MashAutoAdjuster::setSparge`.  Does nothing when the steps already match the
/// method asked for, so that calling it repeatedly does not churn a mash the brewer has set up.
pub fn set_sparge(steps: &mut Vec<Step>, method: SpargeMethod, batches: usize, sparge_temp_c: f64) {
    let batch_sparges = steps.iter().filter(|step| step.step_type == MashStepType::BatchSparge).count();
    let fly_sparges = steps.iter().filter(|step| step.step_type == MashStepType::FlySparge).count();

    let already = match method {
        SpargeMethod::None => batch_sparges == 0 && fly_sparges == 0,
        SpargeMethod::Batch => batch_sparges == batches.max(1) && fly_sparges == 0,
        SpargeMethod::Fly => batch_sparges == 0 && fly_sparges == 1,
    };
    if already {
        return;
    }

    steps.retain(|step| !step.is_sparge());
    let sparge = |name: String, step_type| Step {
        infuse_temp_c: Some(sparge_temp_c),
        ..Step::new(&name, step_type, sparge_temp_c)
    };
    match method {
        SpargeMethod::None => {}
        SpargeMethod::Fly => steps.push(sparge("Sparge".to_owned(), MashStepType::FlySparge)),
        SpargeMethod::Batch => {
            let batches = batches.max(1);
            for batch in 1..=batches {
                // One batch needs no number; several are told apart by one.
                let name = if batches == 1 { "Batch Sparge".to_owned() } else { format!("Batch Sparge {batch}") };
                steps.push(sparge(name, MashStepType::BatchSparge));
            }
        }
    }
}

/// What the mash is being worked out against: the grain, the tun, and how much water the boil
/// needs in total.
///
/// Ported from `MashAutoAdjuster::Inputs`.
#[derive(Debug, Clone, Copy)]
pub struct Inputs {
    pub mashing: Mashing,
    /// Litres of water each kilogram of grain keeps.
    pub grain_absorption_l_kg: f64,
    /// Water that goes in beyond what the thickness calls for -- what is left under the false
    /// bottom, say.
    pub strike_water_allowance_l: f64,
    /// Everything the boil needs, which the mash and the sparge together have to supply.
    pub target_total_water_l: f64,
    pub mash_vessel_volume_l: Option<f64>,
    pub target_collected_wort_l: Option<f64>,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            mashing: Mashing::default(),
            grain_absorption_l_kg: 0.0,
            strike_water_allowance_l: 0.0,
            target_total_water_l: 0.0,
            mash_vessel_volume_l: None,
            target_collected_wort_l: None,
        }
    }
}

/// How thick the mash should be, in litres per kilogram.
///
/// The option where one was given, then the first step's own ratio, then the thickness the
/// strike water already implies, and only then the default.
fn thickness_l_kg(inputs: &Inputs, steps: &[Step], options: &Options) -> Result<f64, String> {
    if let Some(thickness) = options.thickness_l_kg {
        if thickness <= 0.0 {
            return Err("The mash thickness must be more than zero.".to_owned());
        }
        return Ok(thickness);
    }
    if let Some(ratio) = steps.first().and_then(|step| step.liquor_to_grist_ratio_l_kg).filter(|ratio| *ratio > 0.0) {
        return Ok(ratio);
    }
    let strike = steps.first().map(|step| step.amount_l).unwrap_or(0.0);
    if strike > 0.0 && inputs.mashing.grain_kg > 0.0 {
        let implied = (strike - inputs.strike_water_allowance_l).max(0.0) / inputs.mashing.grain_kg;
        if implied > 0.0 {
            return Ok(implied);
        }
    }
    Ok(DEFAULT_THICKNESS_L_KG)
}

/// Work out a mash schedule: what to add, how much of it, and how hot.
///
/// Ported from `MashAutoAdjuster::calculate`.  One pass down the mash steps, carrying how much
/// water is in so far and what temperature the mash is at, then a pass over the sparge steps,
/// which share out whatever water is left.
pub fn calculate(inputs: &Inputs, steps: &[Step], options: &Options) -> Outcome {
    let mut outcome = Outcome { steps: steps.to_vec(), ..Outcome::default() };
    // An output, so whatever was passed in is discarded.
    for step in &mut outcome.steps {
        step.mash_volume_l = None;
    }

    if outcome.steps.is_empty() {
        outcome.error(Scope::Mash, "There are no mash steps.".to_owned());
        return outcome;
    }
    if inputs.mashing.grain_kg <= 0.0 {
        outcome.error(
            Scope::Mash,
            "There are no grains in the mash.  You need some mash-stage fermentable additions before the \
             mash can be calculated."
                .to_owned(),
        );
        return outcome;
    }
    if !inputs.mashing.temperature_makes_sense(inputs.mashing.grain_temp_c) {
        outcome.error(
            Scope::MashProperty("grain_temp"),
            format!(
                "The initial grain temperature is {:.1} °C.  Please set it to the temperature of the grain \
                 before mashing, which is normally room temperature.",
                inputs.mashing.grain_temp_c
            ),
        );
        return outcome;
    }
    if !inputs.mashing.temperature_makes_sense(inputs.mashing.tun_temp_c) {
        outcome.error(
            Scope::MashProperty("initial_tun_temp_c"),
            format!(
                "The initial mash tun temperature is {:.1} °C, which needs to be between freezing and \
                 boiling point ({:.1} °C).",
                inputs.mashing.tun_temp_c, inputs.mashing.boiling_point_c
            ),
        );
        return outcome;
    }

    // The sparge steps are the ones at the end; anything before them is part of the mash.
    let mash_steps = outcome.steps.iter().take_while(|step| !step.is_sparge()).count();
    if outcome.steps[mash_steps..].iter().any(|step| !step.is_sparge()) {
        outcome.error(Scope::Mash, "The sparge steps have to come last.".to_owned());
        return outcome;
    }
    let no_sparge = mash_steps == outcome.steps.len();

    // With no sparge, the last infusion whose amount is not pinned has to make the water up to
    // the total the boil needs -- nothing else can.
    let balancing = if no_sparge {
        (0..mash_steps).rfind(|index| outcome.steps[*index].is_infusion() && !outcome.steps[*index].amount_fixed)
    } else {
        None
    };

    let thickness = match thickness_l_kg(inputs, &outcome.steps, options) {
        Ok(thickness) => thickness,
        Err(message) => {
            outcome.error(Scope::Mash, message);
            return outcome;
        }
    };

    let grain_heat_capacity_kcal_c = inputs.mashing.grain_kg * grain_specific_heat();
    let grain_volume_l = inputs.mashing.grain_kg / constant("grainDensity_kgL");
    let mut water_l = 0.0;
    let mut prior_temp_c = 0.0;
    let mut max_mash_volume_l: f64 = 0.0;

    for index in 0..mash_steps {
        // What the water is being mixed with.  At the first step the grain and the tun are at
        // their own temperatures; afterwards everything has settled to one.
        let contents = if index == 0 {
            inputs.mashing.before_mashing_in()
        } else {
            inputs.mashing.during_mash(water_l, prior_temp_c)
        };

        let outcome_of_step = match outcome.steps[index].step_type {
            MashStepType::Infusion => {
                infusion(&mut outcome.steps, index, &contents, inputs, thickness, water_l, balancing, mash_steps)
            }
            MashStepType::Decoction => decoction(
                &mut outcome.steps,
                index,
                inputs,
                grain_heat_capacity_kcal_c,
                water_l,
                grain_volume_l,
                prior_temp_c,
            ),
            // Nothing to work out: no water goes in and nothing is taken out.
            MashStepType::Temperature | MashStepType::SouringMash | MashStepType::SouringWort => Ok(Vec::new()),
            MashStepType::FlySparge | MashStepType::BatchSparge => unreachable!("sparges come after mash_steps"),
        };
        match outcome_of_step {
            Ok(warnings) => outcome.problems.extend(warnings),
            Err(problem) => {
                outcome.problems.push(problem);
                return outcome;
            }
        }

        if outcome.steps[index].is_infusion() {
            water_l += outcome.steps[index].amount_l;
            // The thickness is recorded on the step.  Not for pinned strike water, where what
            // is on the step is the thickness to aim for when nothing is pinned, and is not to
            // be lost because the brewer has overridden it for now.
            let step = &outcome.steps[index];
            if options.check_only || index > 0 || (!step.amount_fixed && !step.infuse_temp_fixed) {
                outcome.steps[index].liquor_to_grist_ratio_l_kg =
                    Some((water_l - inputs.strike_water_allowance_l).max(0.0) / inputs.mashing.grain_kg);
            }
        }

        prior_temp_c = outcome.steps[index].final_temp_c();
        let mash_volume_l = water_l + grain_volume_l;
        outcome.steps[index].mash_volume_l = Some(mash_volume_l);
        max_mash_volume_l = max_mash_volume_l.max(mash_volume_l);
    }
    outcome.max_mash_volume_l = Some(max_mash_volume_l);

    if let Some(vessel_l) = inputs.mash_vessel_volume_l
        && max_mash_volume_l > vessel_l + VOLUME_TOLERANCE_L
    {
        outcome.warn(
            Scope::Mash,
            format!(
                "At its fullest, the mash will be about {max_mash_volume_l:.1} L, which is more than the \
                 capacity of the mash tun ({vessel_l:.1} L)."
            ),
        );
    }

    spread_sparge(
        &mut outcome,
        inputs,
        options,
        mash_steps,
        water_l,
        grain_heat_capacity_kcal_c,
        prior_temp_c,
        balancing,
    );
    outcome
}

/// Work out one infusion: how much water, and how hot.
///
/// Returns any warnings, or the error that stops the whole schedule.
#[allow(clippy::too_many_arguments)]
fn infusion(
    steps: &mut [Step],
    index: usize,
    contents: &Contents,
    inputs: &Inputs,
    thickness: f64,
    water_so_far_l: f64,
    balancing: Option<usize>,
    mash_steps: usize,
) -> Result<Vec<Problem>, Problem> {
    let error = |property: Option<&'static str>, message: String| Problem {
        severity: Severity::Error,
        scope: Scope::Step(index, property),
        message,
    };
    let target_temp_c = steps[index].start_temp_c;
    let mut warnings = Vec::new();

    let mut water_temp_c = if steps[index].infuse_temp_fixed {
        match steps[index].infuse_temp_c {
            Some(temp) => Some(temp),
            None => {
                return Err(error(
                    Some("infuse_temp_c"),
                    "The infusion temperature is pinned, but has not been set.".to_owned(),
                ));
            }
        }
    } else {
        None
    };

    // How much water, if it is not pinned.
    let amount_l = if steps[index].amount_fixed {
        steps[index].amount_l
    } else if Some(index) == balancing {
        // This step has to make the water up to the total; every later infusion is pinned.
        let mut water_in_other_steps_l = water_so_far_l;
        for later in &steps[(index + 1)..mash_steps] {
            if later.is_infusion() {
                water_in_other_steps_l += later.amount_l;
            }
        }
        let amount = inputs.target_total_water_l - water_in_other_steps_l;
        if amount <= 0.0 {
            return Err(error(
                None,
                format!(
                    "With no sparge, this step has to make up the total water needed ({:.1} L), but the \
                     other steps already add {water_in_other_steps_l:.1} L.",
                    inputs.target_total_water_l
                ),
            ));
        }
        amount
    } else if let Some(temp) = water_temp_c {
        match contents.amount_for(target_temp_c, temp) {
            Some(amount) => amount,
            None => {
                return Err(error(
                    Some("infuse_temp_c"),
                    format!("Cannot reach {target_temp_c:.1} °C with water at {temp:.1} °C."),
                ));
            }
        }
    } else if index == 0 {
        thickness * inputs.mashing.grain_kg + inputs.strike_water_allowance_l
    } else {
        // The hottest water available if heating, the coldest if cooling: either way the one
        // that needs least of it.
        let heating = contents.heat_needed_kcal(target_temp_c) >= 0.0;
        let usual_c = if heating { inputs.mashing.boiling_point_c } else { inputs.mashing.grain_temp_c };
        match contents.amount_for(target_temp_c, usual_c) {
            Some(amount) => {
                water_temp_c = Some(usual_c);
                amount
            }
            None => {
                return Err(error(
                    Some("start_temp_c"),
                    format!("Cannot reach {target_temp_c:.1} °C with water at {usual_c:.1} °C."),
                ));
            }
        }
    };

    if amount_l <= 0.0 {
        return Err(if steps[index].amount_fixed {
            error(Some("amount_l"), "The infusion amount must be more than zero.".to_owned())
        } else {
            // Only reachable when the step needs no heat, ie it is at the same temperature as
            // the one before, in which case how much water goes in is up to the brewer.
            error(
                Some("amount_l"),
                "This infusion does not change the temperature of the mash, so its amount cannot be \
                 calculated.  It needs to be set and pinned."
                    .to_owned(),
            )
        });
    }

    // Both given means neither was worked out from the other, so they may not agree.
    let both_given = water_temp_c.is_some() && (steps[index].amount_fixed || Some(index) == balancing);
    let mut water_temp_c = water_temp_c.unwrap_or_else(|| contents.water_temp_for(target_temp_c, amount_l));

    if water_temp_c > inputs.mashing.boiling_point_c + TEMPERATURE_RANGE_TOLERANCE_C {
        let advice = if index == 0 && !steps[index].amount_fixed && Some(index) != balancing {
            "Try a thinner mash (ie more water)."
        } else {
            "Try more water or a lower step temperature."
        };
        return Err(error(
            Some("infuse_temp_c"),
            format!(
                "The water would need to be at {water_temp_c:.1} °C, which is above boiling point \
                 ({:.1} °C).  {advice}",
                inputs.mashing.boiling_point_c
            ),
        ));
    }
    if !steps[index].infuse_temp_fixed && water_temp_c < inputs.mashing.grain_temp_c - TEMPERATURE_RANGE_TOLERANCE_C {
        // There is no colder water than the cold water there is, which is taken to be at the
        // temperature of the room -- which is also the initial grain temperature.
        let reason = if contents.heat_needed_kcal(target_temp_c) < 0.0 {
            format!("The mash would already be hotter than {target_temp_c:.1} °C at this point.")
        } else {
            format!("This is more water than is needed to reach {target_temp_c:.1} °C.")
        };
        let advice = if steps[index].amount_fixed {
            "Try a smaller pinned amount, or unpinning it."
        } else if Some(index) == balancing {
            "With no sparge, this step has to add the rest of the water needed.  Try having a sparge, or \
             more water in the earlier steps."
        } else {
            "Try a thicker mash (ie less water)."
        };
        return Err(error(
            Some("infuse_temp_c"),
            format!(
                "To reach {target_temp_c:.1} °C with {amount_l:.1} L of water, the water would need to be \
                 at {water_temp_c:.1} °C, which is colder than the coldest water available (taken to be at \
                 the initial grain temperature, {:.1} °C).  {reason}  {advice}",
                inputs.mashing.grain_temp_c
            ),
        ));
    }
    if !steps[index].infuse_temp_fixed {
        // Only just outside the range is rounding, so it is brought back in rather than refused.
        water_temp_c = water_temp_c.clamp(inputs.mashing.grain_temp_c, inputs.mashing.boiling_point_c);
    }

    if both_given {
        let actual_temp_c = contents.final_temp_c(amount_l, water_temp_c);
        if (actual_temp_c - target_temp_c).abs() > TEMPERATURE_TOLERANCE_C {
            warnings.push(Problem {
                severity: Severity::Warning,
                scope: Scope::Step(index, None),
                message: format!(
                    "{amount_l:.1} L of water at {water_temp_c:.1} °C will give a mash temperature of \
                     {actual_temp_c:.1} °C rather than {target_temp_c:.1} °C."
                ),
            });
        }
    }

    steps[index].amount_l = amount_l;
    steps[index].infuse_temp_c = Some(water_temp_c);
    Ok(warnings)
}

/// Work out one decoction: how much of the mash to take out, boil and put back.
///
/// Ported from the decoction branch, whose own comment does the algebra.  Taking out a
/// proportion `r` of the water and grain, boiling it and returning it:
///
/// ```text
///     r × HC_wg × (T_boil - T) = (HC_total - r × HC_wg) × (T - T_prior)
///     r = HC_total × (T - T_prior) / (HC_wg × (T_boil - T_prior))
/// ```
///
/// where `HC_wg` is the heat capacity of the water and grain and `HC_total` includes the tun.
fn decoction(
    steps: &mut [Step],
    index: usize,
    inputs: &Inputs,
    grain_heat_capacity_kcal_c: f64,
    water_l: f64,
    grain_volume_l: f64,
    prior_temp_c: f64,
) -> Result<Vec<Problem>, Problem> {
    let error = |property: Option<&'static str>, message: String| Problem {
        severity: Severity::Error,
        scope: Scope::Step(index, property),
        message,
    };
    let target_temp_c = steps[index].start_temp_c;
    let mut warnings = Vec::new();

    let water_and_grain_kcal_c = grain_heat_capacity_kcal_c + water_l * water_specific_heat();
    let total_kcal_c = water_and_grain_kcal_c + inputs.mashing.tun_heat_capacity_kcal_c;
    let mash_volume_l = water_l + grain_volume_l;
    let boiling_difference_c = inputs.mashing.boiling_point_c - prior_temp_c;
    if boiling_difference_c <= 0.0 {
        return Err(error(None, "The mash is already at boiling point.".to_owned()));
    }

    if steps[index].amount_fixed {
        let ratio = steps[index].amount_l / mash_volume_l;
        if !(0.0..=1.0).contains(&ratio) {
            return Err(error(
                Some("amount_l"),
                format!(
                    "The decoction amount ({:.1} L) must be between zero and the volume of the mash \
                     ({mash_volume_l:.1} L).",
                    steps[index].amount_l
                ),
            ));
        }
        let actual_temp_c = prior_temp_c + ratio * water_and_grain_kcal_c * boiling_difference_c / total_kcal_c;
        if (actual_temp_c - target_temp_c).abs() > TEMPERATURE_TOLERANCE_C {
            warnings.push(Problem {
                severity: Severity::Warning,
                scope: Scope::Step(index, None),
                message: format!(
                    "A decoction of {:.1} L will give a mash temperature of {actual_temp_c:.1} °C rather \
                     than {target_temp_c:.1} °C.",
                    steps[index].amount_l
                ),
            });
        }
    } else {
        if target_temp_c < prior_temp_c {
            return Err(error(
                Some("start_temp_c"),
                "A decoction cannot lower the temperature of the mash.".to_owned(),
            ));
        }
        let ratio = total_kcal_c * (target_temp_c - prior_temp_c) / (water_and_grain_kcal_c * boiling_difference_c);
        if ratio > 1.0 {
            return Err(error(
                Some("start_temp_c"),
                format!("Cannot reach {target_temp_c:.1} °C by decoction, even by boiling the whole mash."),
            ));
        }
        steps[index].amount_l = ratio * mash_volume_l;
    }
    Ok(warnings)
}

/// Share the remaining water out among the sparge steps, and work out how hot each has to be.
///
/// Ported from the second half of `calculate`.  A batch sparge mixes with the drained grain
/// bed, so it is the same heat balance as an infusion; a fly sparge trickles through
/// continuously, where a single mixing calculation does not apply, so the water simply goes in
/// at the temperature the bed is wanted at.
#[allow(clippy::too_many_arguments)]
fn spread_sparge(
    outcome: &mut Outcome,
    inputs: &Inputs,
    options: &Options,
    mash_steps: usize,
    water_l: f64,
    grain_heat_capacity_kcal_c: f64,
    prior_temp_c: f64,
    balancing: Option<usize>,
) {
    let total = outcome.steps.len();
    let collected = |total_water_l: f64| {
        inputs.target_collected_wort_l.map(|target| {
            // What is lost between the water going in and the wort coming out is the difference
            // between the two targets, so it does not need working out again.
            (target - (inputs.target_total_water_l - total_water_l)).max(0.0)
        })
    };

    if mash_steps == total {
        if balancing.is_none() && (water_l - inputs.target_total_water_l).abs() > VOLUME_TOLERANCE_L {
            outcome.warn(
                Scope::Mash,
                format!(
                    "The infusions add up to {water_l:.1} L of water, but {:.1} L is needed in total.",
                    inputs.target_total_water_l
                ),
            );
        }
        outcome.collected_wort_l = collected(water_l);
        return;
    }

    let fixed_sparge_water_l: f64 =
        outcome.steps[mash_steps..].iter().filter(|step| step.amount_fixed).map(|step| step.amount_l).sum();
    let unfixed = outcome.steps[mash_steps..].iter().filter(|step| !step.amount_fixed).count();
    let remaining_l = inputs.target_total_water_l - water_l - fixed_sparge_water_l;

    let mut sparge_amount_l = 0.0;
    if unfixed > 0 {
        if remaining_l < VOLUME_TOLERANCE_L {
            outcome.warn(
                Scope::Mash,
                format!(
                    "There is no water left for sparging: the total water needed is {:.1} L, but the mash \
                     steps already add {water_l:.1} L.  You will collect more wort than needed.  Try a \
                     thicker mash (ie less water).",
                    inputs.target_total_water_l
                ),
            );
        } else {
            sparge_amount_l = remaining_l / unfixed as f64;
        }
    } else if remaining_l.abs() > VOLUME_TOLERANCE_L {
        outcome.warn(
            Scope::Mash,
            format!(
                "The mash and sparge water add up to {:.1} L, but {:.1} L is needed in total.",
                water_l + fixed_sparge_water_l,
                inputs.target_total_water_l
            ),
        );
    }

    // A batch sparge meets the drained bed: the grain, the water it has soaked up, and the tun.
    let bed_heat_capacity_kcal_c = grain_heat_capacity_kcal_c
        + inputs.grain_absorption_l_kg * inputs.mashing.grain_kg * water_specific_heat()
        + inputs.mashing.tun_heat_capacity_kcal_c;

    for index in mash_steps..total {
        if !outcome.steps[index].amount_fixed {
            outcome.steps[index].amount_l = sparge_amount_l;
        }
        let target_temp_c = outcome.steps[index].start_temp_c;
        if outcome.steps[index].infuse_temp_fixed && outcome.steps[index].infuse_temp_c.is_none() {
            outcome.error(
                Scope::Step(index, Some("infuse_temp_c")),
                "The sparge water temperature is pinned, but has not been set.".to_owned(),
            );
            return;
        }

        if outcome.steps[index].step_type == MashStepType::FlySparge {
            // Continuous, so the water is simply at the temperature the bed is wanted at.
            if !outcome.steps[index].infuse_temp_fixed {
                outcome.steps[index].infuse_temp_c = Some(target_temp_c);
            }
        } else if outcome.steps[index].amount_l > 0.0 {
            let bed = Contents {
                parts: vec![Part {
                    heat_capacity_kcal_c: bed_heat_capacity_kcal_c,
                    temp_c: prior_temp_c - options.sparge_heat_loss_c,
                }],
            };
            let amount_l = outcome.steps[index].amount_l;
            if outcome.steps[index].infuse_temp_fixed {
                let water_temp_c = outcome.steps[index].infuse_temp_c.unwrap_or(target_temp_c);
                let actual_temp_c = bed.final_temp_c(amount_l, water_temp_c);
                if (actual_temp_c - target_temp_c).abs() > TEMPERATURE_TOLERANCE_C {
                    outcome.warn(
                        Scope::Step(index, None),
                        format!(
                            "{amount_l:.1} L of water at {water_temp_c:.1} °C will bring the grain bed to \
                             {actual_temp_c:.1} °C rather than {target_temp_c:.1} °C."
                        ),
                    );
                }
            } else {
                let mut water_temp_c = bed.water_temp_for(target_temp_c, amount_l);
                if water_temp_c < target_temp_c {
                    // The bed is already hotter than the step temperature, after a mash out
                    // say.  Cooling it with cold water makes no sense, so the water goes in at
                    // the step temperature and the bed ends up a little warmer.
                    water_temp_c = target_temp_c;
                    let bed_temp_c = bed.final_temp_c(amount_l, water_temp_c);
                    if bed_temp_c > target_temp_c + TEMPERATURE_TOLERANCE_C {
                        outcome.warn(
                            Scope::Step(index, None),
                            format!(
                                "The grain bed is already hot, so it will be at {bed_temp_c:.1} °C rather \
                                 than {target_temp_c:.1} °C."
                            ),
                        );
                    }
                }
                if water_temp_c > inputs.mashing.boiling_point_c {
                    // Not enough water in this batch to heat the bed all the way.  Less
                    // critical for a batch sparge than a fly sparge, so boiling water is used
                    // and the brewer is told where the bed will end up.
                    water_temp_c = inputs.mashing.boiling_point_c;
                    let bed_temp_c = bed.final_temp_c(amount_l, water_temp_c);
                    if bed_temp_c < target_temp_c - TEMPERATURE_TOLERANCE_C {
                        outcome.warn(
                            Scope::Step(index, None),
                            format!(
                                "Even with boiling water the grain bed will only reach {bed_temp_c:.1} °C \
                                 rather than {target_temp_c:.1} °C.  This is usually fine for a batch \
                                 sparge, but you could use fewer batches (ie more water per batch)."
                            ),
                        );
                    }
                }
                outcome.steps[index].infuse_temp_c = Some(water_temp_c);
            }
        } else if !outcome.steps[index].infuse_temp_fixed {
            // No water, so no temperature for it to be at.
            outcome.steps[index].infuse_temp_c = None;
        }

        if let Some(water_temp_c) = outcome.steps[index].infuse_temp_c
            && water_temp_c > inputs.mashing.boiling_point_c + TEMPERATURE_RANGE_TOLERANCE_C
        {
            outcome.error(
                Scope::Step(index, Some("infuse_temp_c")),
                format!(
                    "The sparge water would need to be at {water_temp_c:.1} °C, which is above boiling \
                     point ({:.1} °C).  Try a lower sparge temperature or more sparge water.",
                    inputs.mashing.boiling_point_c
                ),
            );
            return;
        }
    }

    let total_water_l: f64 = water_l + outcome.steps[mash_steps..].iter().map(|step| step.amount_l).sum::<f64>();
    outcome.collected_wort_l = collected(total_water_l);
}

/// Report on a mash without changing it.  `MashAutoAdjuster::check`.
pub fn check(inputs: &Inputs, steps: &[Step]) -> Outcome {
    calculate(inputs, steps, &Options { check_only: true, ..Options::default() })
}
