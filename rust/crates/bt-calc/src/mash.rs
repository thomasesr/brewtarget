// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/mash.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! How fermentable a mash makes the wort.
//!
//! Ported from `src/calculations/MashFermentability.cpp`, which implements a published kinetic
//! model of the mash: starch gelatinises at a temperature-dependent rate, alpha and beta amylase
//! break it down into glucose, maltose, maltotriose and dextrins at their own rates, and both
//! enzymes denature as they work.  Stepping that forward in time says what fraction of the
//! dissolved extract the yeast can actually ferment.
//!
//! The constants are the paper's, with the C++'s own symbol names kept in the comments.

/// The gas constant, as the C++ rounds it.
const R: f64 = 8.31;
const KELVIN_OFFSET: f64 = 273.15;

/// `ln` of an Arrhenius rate, which is how the C++ interpolates between two of them.
fn ln_arrhenius(pre_exponential_factor: f64, activation_energy_j_per_mol: f64, temp_k: f64) -> f64 {
    pre_exponential_factor.ln() - activation_energy_j_per_mol / (R * temp_k)
}

fn arrhenius(pre_exponential_factor: f64, activation_energy_j_per_mol: f64, temp_k: f64) -> f64 {
    ln_arrhenius(pre_exponential_factor, activation_energy_j_per_mol, temp_k).exp()
}

// Gelatinisation: the paper gives two Arrhenius lines, one below 60 °C and one from 63 °C up.
const GELATINISATION_THRESHOLD_C: f64 = 60.0; // Tg
const GELATINISATION_UPPER_C: f64 = 63.0;
const GELATINISATION_BELOW_PRE_EXP: f64 = 5.7e31; // kg1, per second
const GELATINISATION_BELOW_J_PER_MOL: f64 = 220.6e3; // Eg1
const GELATINISATION_ABOVE_PRE_EXP: f64 = 3.1e14; // kg2, per second
const GELATINISATION_ABOVE_J_PER_MOL: f64 = 108.3e3; // Eg2

/// Between the two lines the rate is interpolated in log space, so the curve stays continuous.
fn gelatinisation_rate_per_sec(temp_c: f64) -> f64 {
    let temp_k = temp_c + KELVIN_OFFSET;
    if temp_c <= GELATINISATION_THRESHOLD_C {
        return arrhenius(GELATINISATION_BELOW_PRE_EXP, GELATINISATION_BELOW_J_PER_MOL, temp_k);
    }
    if temp_c >= GELATINISATION_UPPER_C {
        return arrhenius(GELATINISATION_ABOVE_PRE_EXP, GELATINISATION_ABOVE_J_PER_MOL, temp_k);
    }
    let ln_rate_at_lower = ln_arrhenius(
        GELATINISATION_BELOW_PRE_EXP,
        GELATINISATION_BELOW_J_PER_MOL,
        GELATINISATION_THRESHOLD_C + KELVIN_OFFSET,
    );
    let ln_rate_at_upper = ln_arrhenius(
        GELATINISATION_ABOVE_PRE_EXP,
        GELATINISATION_ABOVE_J_PER_MOL,
        GELATINISATION_UPPER_C + KELVIN_OFFSET,
    );
    let fraction = (temp_c - GELATINISATION_THRESHOLD_C) / (GELATINISATION_UPPER_C - GELATINISATION_THRESHOLD_C);
    (ln_rate_at_lower + fraction * (ln_rate_at_upper - ln_rate_at_lower)).exp()
}

const ALPHA_DENATURATION_PRE_EXP: f64 = 6.9e30; // kdα, per second
const ALPHA_DENATURATION_J_PER_MOL: f64 = 224.2e3; // Edα
const BETA_DENATURATION_PRE_EXP: f64 = 7.6e60; // kdβ, per second
const BETA_DENATURATION_J_PER_MOL: f64 = 410.7e3; // Edβ

/// Alpha amylase activity against temperature, as the paper measured it, in kelvin.
const ALPHA_RELATIVE_ACTIVITY: [(f64, f64); 18] = [
    (310.0, 1.0),
    (313.0, 1.0),
    (318.0, 1.6),
    (323.0, 3.7),
    (325.0, 4.5),
    (326.0, 4.9),
    (328.0, 5.8),
    (329.0, 6.1),
    (333.0, 7.0),
    (336.0, 7.2),
    (338.0, 6.6),
    (339.0, 6.2),
    (340.0, 5.5),
    (341.0, 4.8),
    (343.0, 3.1),
    (344.0, 2.2),
    (345.0, 1.4),
    (347.0, 0.0),
];

fn alpha_relative_specific_activity(temp_c: f64) -> f64 {
    let temp_k = temp_c + KELVIN_OFFSET;
    let first = ALPHA_RELATIVE_ACTIVITY[0];
    let last = ALPHA_RELATIVE_ACTIVITY[ALPHA_RELATIVE_ACTIVITY.len() - 1];
    if temp_k <= first.0 {
        return first.1;
    }
    if temp_k >= last.0 {
        return last.1;
    }
    for index in 1..ALPHA_RELATIVE_ACTIVITY.len() {
        let upper = ALPHA_RELATIVE_ACTIVITY[index];
        if temp_k <= upper.0 {
            let lower = ALPHA_RELATIVE_ACTIVITY[index - 1];
            let fraction = (temp_k - lower.0) / (upper.0 - lower.0);
            return lower.1 + fraction * (upper.1 - lower.1);
        }
    }
    0.0
}

/// Beta amylase: two straight lines, rising to 336 K and falling to nothing by 343 K.
fn beta_relative_specific_activity(temp_c: f64) -> f64 {
    let temp_k = temp_c + KELVIN_OFFSET;
    if temp_k <= 336.0 {
        return (0.049 * temp_k - 13.9).max(0.0);
    }
    if temp_k >= 343.0 {
        return 0.0;
    }
    (-0.374 * temp_k + 128.3).max(0.0)
}

// Rates at which the enzymes convert starch, per the paper's symbols.
const STARCH_TO_GLUCOSE_BY_ALPHA: f64 = 0.023; // kgl
const STARCH_TO_MALTOSE_BY_ALPHA: f64 = 0.389; // kα,mal
const STARCH_TO_MALTOSE_BY_BETA: f64 = 0.137; // kβ,mal
const STARCH_TO_MALTOTRIOSE_BY_ALPHA: f64 = 0.117; // kmlt
const STARCH_TO_DEXTRINS_BY_ALPHA: f64 = 0.317; // kdex
const DEXTRIN_TO_GLUCOSE_BY_ALPHA: f64 = 2.9e-8; // k'gl
const DEXTRIN_TO_MALTOSE_BY_ALPHA: f64 = 1.2e-7; // k'α,mal
const DEXTRIN_TO_MALTOSE_BY_BETA: f64 = 8.4e-8; // k'β,mal
const DEXTRIN_TO_MALTOTRIOSE_BY_ALPHA: f64 = 1.5e-8; // k'mlt

// Starch takes on water as it is broken into sugars, so a gram of starch becomes rather more
// than a gram of sugar.
const HYDRATION_FACTOR_GLUCOSE: f64 = 180.16 / 162.14;
const HYDRATION_FACTOR_MALTOSE: f64 = 342.30 / 324.28;
const HYDRATION_FACTOR_MALTOTRIOSE: f64 = 504.44 / 486.42;

const INITIAL_STARCH_G_PER_KG: f64 = 113.5;
const INITIAL_GLUCOSE_G_PER_KG: f64 = 4.0; // Sugars already in the malt
const INITIAL_MALTOSE_G_PER_KG: f64 = 5.0;
const INITIAL_ALPHA_U_PER_KG: f64 = 210.0;
const INITIAL_BETA_U_PER_KG: f64 = 380.0;

// The range of malts the paper actually tested, which bounds how far the model is trusted.
const MIN_TESTED_ALPHA_U_PER_KG: f64 = 110.0; // Malt M1
const MAX_TESTED_ALPHA_U_PER_KG: f64 = 290.0; // Malt S3
const MIN_TESTED_BETA_U_PER_KG: f64 = 345.0; // Malt M1
const MAX_TESTED_BETA_U_PER_KG: f64 = 550.0; // Malt M2

const TIME_STEP_SECS: f64 = 15.0;
const MIN_TEMP_C: f64 = 0.0;
const MAX_TEMP_C: f64 = 100.0;
const MAX_DURATION_MINS: f64 = 24.0 * 60.0;

/// Nominal diastatic power, in Lintner, converted from the 250 WK the paper's malt had.
const NOMINAL_DIASTATIC_POWER_LINTNER: f64 = (250.0 + 16.0) / 3.5;
const PAPER_MASH_THICKNESS_L_KG: f64 = 70.0 / 20.0;
const MIN_SCALED_MASH_THICKNESS_L_KG: f64 = 2.0;
const MAX_SCALED_MASH_THICKNESS_L_KG: f64 = 5.0;
const REFERENCE_MASH_THICKNESS_L_KG: f64 = 3.0;
const REFERENCE_MASH_TEMP_C: f64 = 66.0;
const REFERENCE_MASH_TIME_MINS: f64 = 60.0;

const DEFAULT_HEATING_RATE_C_PER_MIN: f64 = 1.0;
const MASH_OUT_TEMP_C: f64 = 76.0;
const MASH_OUT_HOLD_MINS: f64 = 10.0;

const MIN_ATTENUATION_MULTIPLIER: f64 = 0.81;
const MAX_ATTENUATION_MULTIPLIER: f64 = 1.20;
const MAX_MASH_ADJUSTED_ATTENUATION_PCT: f64 = 95.0;

/// What kind of mash step this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepType {
    Infusion,
    /// Heated directly, so the ramp time depends on how far it has to climb.
    Temperature,
    Decoction,
    /// Sparging ends the mash as far as this model is concerned.
    Sparge,
    /// Anything else, which is skipped.
    Other,
}

/// One step of a mash.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: StepType,
    pub step_temp_c: f64,
    /// Where the step ends up, if it drifts; defaults to the step temperature.
    pub end_temp_c: Option<f64>,
    /// How long the step is held, not counting the ramp.
    pub step_time_mins: f64,
    pub ramp_time_mins: Option<f64>,
}

/// A point on the mash's temperature-against-time curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfilePoint {
    pub time_mins: f64,
    pub temp_c: f64,
}

/// How much enzyme the malt brings, relative to the paper's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnzymeLevels {
    pub alpha: f64,
    pub beta: f64,
}

impl Default for EnzymeLevels {
    fn default() -> Self {
        Self { alpha: 1.0, beta: 1.0 }
    }
}

/// What is left, and what has been made, at the end of the mash.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Result {
    pub ungelatinised_starch_g_per_kg: f64,
    pub gelatinised_starch_g_per_kg: f64,
    pub dextrins_g_per_kg: f64,
    pub glucose_g_per_kg: f64,
    pub maltose_g_per_kg: f64,
    pub maltotriose_g_per_kg: f64,
}

impl Result {
    pub fn fermentable_sugars_g_per_kg(&self) -> f64 {
        self.glucose_g_per_kg + self.maltose_g_per_kg + self.maltotriose_g_per_kg
    }

    /// The share of the dissolved extract that the yeast can ferment.
    pub fn fermentable_fraction(&self) -> f64 {
        let sugars = self.fermentable_sugars_g_per_kg();
        let dissolved_extract = sugars + self.dextrins_g_per_kg + self.gelatinised_starch_g_per_kg;
        if dissolved_extract > 0.0 { sugars / dissolved_extract } else { 0.0 }
    }

    /// How much of the starch made it into solution at all.
    pub fn fraction_of_starch_converted(&self) -> f64 {
        1.0 - (self.ungelatinised_starch_g_per_kg + self.gelatinised_starch_g_per_kg) / INITIAL_STARCH_G_PER_KG
    }
}

/// The temperature at a point in time, interpolating between profile points.
fn temperature_at(profile: &[ProfilePoint], time_mins: f64, hint: &mut usize) -> f64 {
    while *hint + 1 < profile.len() && profile[*hint + 1].time_mins < time_mins {
        *hint += 1;
    }
    if *hint + 1 >= profile.len() {
        return profile.last().expect("profile is not empty").temp_c;
    }
    let start = profile[*hint];
    let end = profile[*hint + 1];
    let span_mins = end.time_mins - start.time_mins;
    if span_mins <= 0.0 {
        return end.temp_c;
    }
    let fraction = ((time_mins - start.time_mins) / span_mins).clamp(0.0, 1.0);
    start.temp_c + fraction * (end.temp_c - start.temp_c)
}

fn append_point(profile: &mut Vec<ProfilePoint>, duration_mins: f64, temp_c: f64) {
    let last_time_mins = profile.last().map_or(0.0, |point| point.time_mins);
    profile.push(ProfilePoint {
        time_mins: last_time_mins + duration_mins.clamp(0.0, MAX_DURATION_MINS),
        temp_c: temp_c.clamp(MIN_TEMP_C, MAX_TEMP_C),
    });
}

/// Turn a list of mash steps into a temperature profile.
///
/// A sparge ends the mash; anything the model does not recognise is skipped.  A mash that does
/// not already reach mash-out temperature gets one added, since that is what the enzymes
/// actually see.
pub fn build_profile(steps: &[Step]) -> Vec<ProfilePoint> {
    let mut profile: Vec<ProfilePoint> = Vec::new();
    for step in steps {
        match step.kind {
            StepType::Sparge => break,
            StepType::Other => continue,
            _ => {}
        }
        let step_temp_c = step.step_temp_c.clamp(MIN_TEMP_C, MAX_TEMP_C);
        if profile.is_empty() {
            profile.push(ProfilePoint { time_mins: 0.0, temp_c: step_temp_c });
        } else {
            let previous_temp_c = profile.last().expect("not empty").temp_c;
            let ramp_time_mins = match step.ramp_time_mins {
                Some(ramp) => ramp,
                None if step.kind == StepType::Temperature => {
                    (step_temp_c - previous_temp_c).abs() / DEFAULT_HEATING_RATE_C_PER_MIN
                }
                None => 0.0,
            };
            append_point(&mut profile, ramp_time_mins, step_temp_c);
        }
        append_point(&mut profile, step.step_time_mins, step.end_temp_c.unwrap_or(step_temp_c));
    }

    if profile.is_empty() {
        return profile;
    }
    let final_temp_c = profile.last().expect("not empty").temp_c;
    if final_temp_c < MASH_OUT_TEMP_C {
        append_point(&mut profile, (MASH_OUT_TEMP_C - final_temp_c) / DEFAULT_HEATING_RATE_C_PER_MIN, MASH_OUT_TEMP_C);
        append_point(&mut profile, MASH_OUT_HOLD_MINS, MASH_OUT_TEMP_C);
    }
    profile
}

/// How much enzyme this malt and mash thickness bring, relative to the paper's.
///
/// Both are clamped to the range of malts the paper actually tested, so an unusual diastatic
/// power does not extrapolate the model somewhere it was never checked.
pub fn enzyme_levels(diastatic_power_lintner: Option<f64>, mash_thickness_l_kg: Option<f64>) -> EnzymeLevels {
    let diastatic_power_ratio =
        diastatic_power_lintner.unwrap_or(NOMINAL_DIASTATIC_POWER_LINTNER).max(0.0) / NOMINAL_DIASTATIC_POWER_LINTNER;
    let alpha_for_diastatic_power = diastatic_power_ratio
        .clamp(MIN_TESTED_ALPHA_U_PER_KG / INITIAL_ALPHA_U_PER_KG, MAX_TESTED_ALPHA_U_PER_KG / INITIAL_ALPHA_U_PER_KG);
    let beta_for_diastatic_power = diastatic_power_ratio
        .clamp(MIN_TESTED_BETA_U_PER_KG / INITIAL_BETA_U_PER_KG, MAX_TESTED_BETA_U_PER_KG / INITIAL_BETA_U_PER_KG);

    // A thinner mash dilutes the enzymes, so activity scales with concentration.
    let clamped_thickness_l_kg = mash_thickness_l_kg
        .unwrap_or(REFERENCE_MASH_THICKNESS_L_KG)
        .clamp(MIN_SCALED_MASH_THICKNESS_L_KG, MAX_SCALED_MASH_THICKNESS_L_KG);
    let concentration_for_thickness = PAPER_MASH_THICKNESS_L_KG / clamped_thickness_l_kg;

    EnzymeLevels {
        alpha: alpha_for_diastatic_power * concentration_for_thickness,
        beta: beta_for_diastatic_power * concentration_for_thickness,
    }
}

/// Step the mash forward in time and report what the enzymes have made of the starch.
///
/// Ported from `MashFermentability::simulate`.  Each fifteen-second step gelatinises some of the
/// remaining starch, breaks down some of what is already gelatinised, works on the dextrins, and
/// denatures some of each enzyme.  Rates are evaluated at the midpoint of the step.
pub fn simulate(profile: &[ProfilePoint], enzyme_levels: EnzymeLevels) -> Result {
    let mut ungelatinised_starch = INITIAL_STARCH_G_PER_KG;
    let mut gelatinised_starch = 0.0;
    let mut dextrins = 0.0;
    let mut glucose = 0.0;
    let mut maltose = 0.0;
    let mut maltotriose = 0.0;
    let mut intact_alpha = INITIAL_ALPHA_U_PER_KG * enzyme_levels.alpha;
    let mut intact_beta = INITIAL_BETA_U_PER_KG * enzyme_levels.beta;

    let total_time_secs = profile.last().map_or(0.0, |point| point.time_mins * 60.0);
    let mut profile_hint = 0usize;
    let mut time_secs = 0.0;
    while time_secs < total_time_secs {
        let dt_secs = TIME_STEP_SECS.min(total_time_secs - time_secs);
        let temp_c = temperature_at(profile, (time_secs + dt_secs / 2.0) / 60.0, &mut profile_hint);
        let alpha = intact_alpha * alpha_relative_specific_activity(temp_c);
        let beta = intact_beta * beta_relative_specific_activity(temp_c);

        let newly_gelatinised = ungelatinised_starch * (1.0 - (-gelatinisation_rate_per_sec(temp_c) * dt_secs).exp());
        ungelatinised_starch -= newly_gelatinised;

        let to_glucose = STARCH_TO_GLUCOSE_BY_ALPHA * alpha;
        let to_maltose = STARCH_TO_MALTOSE_BY_ALPHA * alpha + STARCH_TO_MALTOSE_BY_BETA * beta;
        let to_maltotriose = STARCH_TO_MALTOTRIOSE_BY_ALPHA * alpha;
        let to_dextrins = STARCH_TO_DEXTRINS_BY_ALPHA * alpha;
        let starch_rate = to_glucose + to_maltose + to_maltotriose + to_dextrins;

        let mut new_dextrins = 0.0;
        gelatinised_starch += newly_gelatinised;
        if starch_rate > 0.0 {
            let broken_down = gelatinised_starch * (1.0 - (-starch_rate * dt_secs).exp());
            gelatinised_starch -= broken_down;
            glucose += broken_down * to_glucose / starch_rate;
            maltose += broken_down * to_maltose / starch_rate;
            maltotriose += broken_down * to_maltotriose / starch_rate;
            new_dextrins = broken_down * to_dextrins / starch_rate;
        }

        let dex_to_glucose = DEXTRIN_TO_GLUCOSE_BY_ALPHA * alpha;
        let dex_to_maltose = DEXTRIN_TO_MALTOSE_BY_ALPHA * alpha + DEXTRIN_TO_MALTOSE_BY_BETA * beta;
        let dex_to_maltotriose = DEXTRIN_TO_MALTOTRIOSE_BY_ALPHA * alpha;
        let dextrin_rate = dex_to_glucose + dex_to_maltose + dex_to_maltotriose;
        if dextrin_rate > 0.0 {
            let broken_down = dextrins * (1.0 - (-dextrin_rate * dt_secs).exp());
            dextrins -= broken_down;
            glucose += broken_down * dex_to_glucose / dextrin_rate;
            maltose += broken_down * dex_to_maltose / dextrin_rate;
            maltotriose += broken_down * dex_to_maltotriose / dextrin_rate;
        }
        // Dextrins made this step are added after, so they are not broken down in the same step.
        dextrins += new_dextrins;

        let temp_k = temp_c + KELVIN_OFFSET;
        intact_alpha *= (-arrhenius(ALPHA_DENATURATION_PRE_EXP, ALPHA_DENATURATION_J_PER_MOL, temp_k) * dt_secs).exp();
        intact_beta *= (-arrhenius(BETA_DENATURATION_PRE_EXP, BETA_DENATURATION_J_PER_MOL, temp_k) * dt_secs).exp();

        time_secs += TIME_STEP_SECS;
    }

    Result {
        ungelatinised_starch_g_per_kg: ungelatinised_starch,
        gelatinised_starch_g_per_kg: gelatinised_starch,
        dextrins_g_per_kg: dextrins,
        glucose_g_per_kg: INITIAL_GLUCOSE_G_PER_KG + glucose * HYDRATION_FACTOR_GLUCOSE,
        maltose_g_per_kg: INITIAL_MALTOSE_G_PER_KG + maltose * HYDRATION_FACTOR_MALTOSE,
        maltotriose_g_per_kg: maltotriose * HYDRATION_FACTOR_MALTOTRIOSE,
    }
}

/// The fermentable fraction of a typical single infusion, which everything else is measured
/// against: 66 °C for an hour at the reference thickness.
pub fn reference_fermentable_fraction() -> f64 {
    static REFERENCE: std::sync::OnceLock<f64> = std::sync::OnceLock::new();
    *REFERENCE.get_or_init(|| {
        let profile = build_profile(&[Step {
            kind: StepType::Infusion,
            step_temp_c: REFERENCE_MASH_TEMP_C,
            end_temp_c: None,
            step_time_mins: REFERENCE_MASH_TIME_MINS,
            ramp_time_mins: None,
        }]);
        simulate(&profile, enzyme_levels(None, Some(REFERENCE_MASH_THICKNESS_L_KG))).fermentable_fraction()
    })
}

/// How much more or less attenuation this mash gives than the reference one.
///
/// Clamped, because the model's absolute calibration is less trustworthy than its shape -- the
/// point is the difference between mashes, not the figure itself.
pub fn attenuation_multiplier(profile: &[ProfilePoint], enzyme_levels: EnzymeLevels) -> Option<f64> {
    if profile.is_empty() {
        return None;
    }
    let multiplier = simulate(profile, enzyme_levels).fermentable_fraction() / reference_fermentable_fraction();
    Some(multiplier.clamp(MIN_ATTENUATION_MULTIPLIER, MAX_ATTENUATION_MULTIPLIER))
}

/// Apply the multiplier to a yeast's published attenuation.
///
/// Never pushes attenuation above the cap, but never pulls a yeast's own higher figure down to it
/// either.
pub fn adjusted_attenuation_pct(yeast_attenuation_pct: f64, multiplier: f64) -> f64 {
    let adjusted = yeast_attenuation_pct * multiplier;
    adjusted.min(yeast_attenuation_pct.max(MAX_MASH_ADJUSTED_ATTENUATION_PCT))
}
