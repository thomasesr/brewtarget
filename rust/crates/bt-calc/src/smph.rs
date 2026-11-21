// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/smph.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Hosom's SMPH model of bitterness.
//!
//! Ported from the `Smph` namespace and the `smph` function in
//! `src/calculations/IbuMethods.cpp`.
//!
//! The other methods stop at the kettle.  SMPH follows the bitterness all the way to the glass:
//! alpha acids dissolve, isomerise into iso-alpha acids at a temperature-dependent rate, and are
//! then lost to the boil, to trub, to the yeast, to the krausen and to time in the bottle.  It
//! also counts the bitterness that is not iso-alpha acid at all -- oxidised alpha and beta acids,
//! and polyphenols -- which is why its breakdown has a third component the others leave at zero.
//!
//! The constants carry the section and reference numbers from Hosom's paper, as the C++ does.

use bt_model::enums::{FermentationKrausenHandling, QualitativeWortClarity, YeastFlocculation};

use crate::ibu::{HopForm, Parameters, natural_cooling_rate, post_boil_wort_temperature_c, relative_utilization};

pub const LOSS_FACTOR_BOIL: f64 = 0.51;
pub const OAA_BOIL_FACTOR: f64 = 0.11;
pub const OAA_AGE_SCALE: f64 = 0.33;
const ALPHA_ACID_LIMIT_MIN_PPM: f64 = 200.0;
const ALPHA_ACID_LIMIT_MAX_PPM: f64 = 580.0;
/// Section 3.3.
const LOSS_FACTOR_FERMENT: f64 = 0.85;
/// Reference [38].
const SCALE_OAA: f64 = 0.9155;
/// The calculator uses this; the paper's [40] says 0.071.
const OBA_BOIL_FACTOR: f64 = 0.07125;
/// Reference [44].
const SCALE_OBA: f64 = 0.85;
/// Section 3.4.4.
const DEFAULT_POLYPHENOL_RATING: f64 = 0.04;
/// Reference [48].
const LOSS_FACTOR_PP_PRECIPITATE: f64 = 0.20;
/// Section 3.4.4.
const LOSS_FACTOR_PP_FERMENT: f64 = 0.70;
/// F(form) in [35], Section 3.4.1: pellets oxidise twice as readily.
const PELLET_OAA_FACTOR: f64 = 2.0;
/// Reference [35].
const FRESH_HOPS_DRYING_DAYS: f64 = 3.5;
/// An average over many varieties.
const DEFAULT_SIX_MONTH_ALPHA_LOSS: f64 = 0.32;
/// The pH at which every pH factor comes out at 1.0.
pub const DEFAULT_POST_BOIL_PH: f64 = 5.75;
/// For malt polyphenols.
const UNADJUSTED_PH: f64 = 5.75;
/// Below this the wort is taken to have stopped isomerising altogether.
const MINIMUM_ISOMERIZATION_TEMP_C: f64 = 50.0;
/// The calculator's figure; the paper's [4] says 5.0/7.0.
const IBU_PER_IAA_PPM: f64 = 50.0 / 69.68;
/// Reference [52], via the calculator.
const SCALE_PP_HOPS: f64 = 0.022 / IBU_PER_IAA_PPM;
/// Brewtarget does not track how old the beer is or how fresh the hops were, so both are held at
/// the value that makes their factor 1.0 rather than guessed at.
const BEER_AGE_WEEKS: f64 = 1.0;
const HOP_FRESHNESS_FACTOR: f64 = 1.0;
const DEFAULT_BOILING_POINT_C: f64 = 100.0;
const ASSUMED_KETTLE_DIAMETER_CM: f64 = 45.0;
/// The step the post-boil integration takes, in minutes.
const POST_BOIL_STEP_MINUTES: f64 = 0.01;

/// Rate at which alpha acids isomerise.
fn k1(temp_k: f64) -> f64 {
    7.9e11 * (-11858.0 / temp_k).exp()
}

/// Rate at which the iso-alpha acids themselves break down.
fn k2(temp_k: f64) -> f64 {
    4.1e12 * (-12994.0 / temp_k).exp()
}

/// How much alpha acid is in solution after adding more.
///
/// Wort can only hold so much: past about 200 ppm each further addition dissolves less than the
/// last, approaching a ceiling near 580 ppm.  Ported from `Smph::dissolvedAfterAddition_ppm`,
/// which inverts the saturation curve to find where the existing amount sits on it before adding.
pub fn dissolved_after_addition_ppm(already_dissolved_ppm: f64, added_ppm: f64) -> f64 {
    let slope = -(1.0 - (ALPHA_ACID_LIMIT_MIN_PPM / ALPHA_ACID_LIMIT_MAX_PPM)).ln() / ALPHA_ACID_LIMIT_MIN_PPM;

    let mut without_limit_ppm = already_dissolved_ppm;
    if already_dissolved_ppm > ALPHA_ACID_LIMIT_MIN_PPM {
        without_limit_ppm = if already_dissolved_ppm < ALPHA_ACID_LIMIT_MAX_PPM {
            -(1.0 - already_dissolved_ppm / ALPHA_ACID_LIMIT_MAX_PPM).ln() / slope
        } else {
            f64::MAX / 2.0
        };
    }
    without_limit_ppm += added_ppm;
    if without_limit_ppm <= ALPHA_ACID_LIMIT_MIN_PPM {
        return without_limit_ppm;
    }
    ALPHA_ACID_LIMIT_MAX_PPM * (1.0 - (-slope * without_limit_ppm).exp())
}

/// Stronger wort holds on to less of its bitterness, but only once it has steeped a while.
pub fn loss_factor_gravity(original_gravity_sg: f64, effective_steep_time_minutes: f64) -> f64 {
    let points = original_gravity_sg - 1.0;
    if points <= 0.0 || effective_steep_time_minutes <= 30.0 {
        return 1.0;
    }
    let s =
        if effective_steep_time_minutes >= 40.0 { 4.91 } else { 0.391 * (effective_steep_time_minutes - 30.0) + 1.0 };
    1.0 - 2.0 * (-1.0 / (s * points)).exp()
}

/// Bitterness fades as the beer ages, levelling off after about fifteen weeks.
pub fn loss_factor_age(age_weeks: f64) -> f64 {
    0.35 * (-0.073 * age_weeks.min(15.0)).exp() + 0.65
}

pub fn loss_factor_ph_iaa(ph: f64) -> f64 {
    (0.071 * ph + 0.592).max(0.0)
}

pub fn loss_factor_ph_oaa(ph: f64) -> f64 {
    (1.178506 * ph - 5.776411).max(0.0)
}

/// Clearer wort carries more bitterness through; trub takes it with it.
pub fn loss_factor_wort_clarity(wort_clarity: Option<QualitativeWortClarity>) -> f64 {
    match wort_clarity {
        Some(QualitativeWortClarity::VeryClear) => 1.30,
        Some(QualitativeWortClarity::Clear) => 1.20,
        Some(QualitativeWortClarity::SomewhatClear) => 1.10,
        Some(QualitativeWortClarity::Average) | None => 1.00,
        Some(QualitativeWortClarity::SomewhatCloudy) => 0.90,
        Some(QualitativeWortClarity::Cloudy) => 0.80,
        Some(QualitativeWortClarity::VeryCloudy) => 0.70,
        Some(QualitativeWortClarity::ExtremelyCloudy) => 0.60,
    }
}

/// Krausen carries iso-alpha acids out of the beer, so blowing it off costs bitterness.
pub fn loss_factor_krausen_iaa(krausen_handling: Option<FermentationKrausenHandling>) -> f64 {
    match krausen_handling {
        Some(FermentationKrausenHandling::MixedIn) => 1.1268, // = 1.0/0.8875
        Some(FermentationKrausenHandling::MinorDeposits) => 1.05,
        Some(FermentationKrausenHandling::MediumDeposits) | None => 1.00,
        Some(FermentationKrausenHandling::HeavyDeposits) => 0.95,
        Some(FermentationKrausenHandling::VeryHeavyDeposits) => 0.90,
        Some(FermentationKrausenHandling::BlownOffSlowFermentation) => 0.938,
        Some(FermentationKrausenHandling::BlownOffNormalFermentation) => 0.833,
        Some(FermentationKrausenHandling::BlownOffVigorousFermentation) => 0.729,
    }
}

/// The non-iso-alpha-acid bitterness is three times as sensitive to the krausen.
pub fn loss_factor_krausen_non_iaa(krausen_handling: Option<FermentationKrausenHandling>) -> f64 {
    const NON_IAA_KRAUSEN_FACTOR: f64 = 3.0;
    1.0 - NON_IAA_KRAUSEN_FACTOR * (1.0 - loss_factor_krausen_iaa(krausen_handling))
}

/// A flocculent yeast drags more bitterness down with it.
pub fn loss_factor_flocculation(flocculation: Option<YeastFlocculation>) -> f64 {
    match flocculation {
        Some(YeastFlocculation::VeryLow | YeastFlocculation::Low) => 1.05,
        Some(YeastFlocculation::High | YeastFlocculation::VeryHigh) => 0.95,
        _ => 1.00,
    }
}

/// How much of the alpha acid had already oxidised before it reached the kettle.
pub fn pre_kettle_alpha_acid_oxidation(six_month_alpha_loss: Option<f64>) -> f64 {
    let loss = six_month_alpha_loss.unwrap_or(DEFAULT_SIX_MONTH_ALPHA_LOSS).clamp(0.0, 0.99);
    let k_per_day = -(1.0 - loss).ln() / 182.5;
    let oaa_fresh = 1.0 - (-k_per_day * FRESH_HOPS_DRYING_DAYS).exp();
    oaa_fresh + OAA_AGE_SCALE * (1.0 - HOP_FRESHNESS_FACTOR)
}

/// Step the isomerisation forward.
///
/// Ported from `Smph::advance`.  Alpha acids decay into iso-alpha acids at `k1` while the
/// iso-alpha acids themselves break down at `k2`, so this is the analytic solution of that pair
/// of reactions over one interval at a fixed temperature.  When the two rates coincide the
/// closed form divides by zero, and the limiting case is used instead -- which is what the C++'s
/// `1.0e-12` guard is for.
pub fn advance(alpha_acids_ppm: &mut f64, iaa_ppm: &mut f64, temp_k: f64, duration_minutes: f64) {
    let rate_k1 = k1(temp_k);
    let rate_k2 = k2(temp_k);
    let decay_aa = (-rate_k1 * duration_minutes).exp();
    let decay_iaa = (-rate_k2 * duration_minutes).exp();

    let formed_iaa_ppm = if (rate_k2 - rate_k1).abs() > 1.0e-12 {
        *alpha_acids_ppm * rate_k1 / (rate_k2 - rate_k1) * (decay_aa - decay_iaa)
    } else {
        *alpha_acids_ppm * rate_k1 * duration_minutes * decay_aa
    };
    *iaa_ppm = *iaa_ppm * decay_iaa + formed_iaa_ppm;
    *alpha_acids_ppm *= decay_aa;
}

/// Everything SMPH needs beyond what the other methods do.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SmphParameters {
    /// Where the wort boils, which is below 100 °C at altitude.
    pub boiling_point_c: Option<f64>,
    pub post_boil_ph: Option<f64>,
    /// How much alpha acid was already in the kettle when this hop went in, which limits how much
    /// more can dissolve.
    pub kettle_dissolved_alpha_acids_ppm: Option<f64>,
    pub kettle_volume_at_addition_liters: Option<f64>,
    /// The fraction of its alpha acid this hop loses in six months at room temperature.
    pub six_month_alpha_loss: Option<f64>,
    pub beta_acid_rating: Option<f64>,
    pub polyphenol_rating: Option<f64>,
    pub yeast_flocculation: Option<YeastFlocculation>,
    pub wort_clarity: Option<QualitativeWortClarity>,
    pub krausen_handling: Option<FermentationKrausenHandling>,
}

fn boiling_point_c(smph: &SmphParameters) -> f64 {
    smph.boiling_point_c.unwrap_or(DEFAULT_BOILING_POINT_C)
}

fn cooling_rate(parameters: &Parameters) -> f64 {
    natural_cooling_rate(
        parameters.kettle_volume_at_flameout_liters.unwrap_or(parameters.post_boil_volume_liters),
        parameters.kettle_internal_diameter_cm.unwrap_or(ASSUMED_KETTLE_DIAMETER_CM),
        parameters.kettle_opening_diameter_cm.unwrap_or(ASSUMED_KETTLE_DIAMETER_CM),
    )
}

/// The bitterness a recipe gets from its malt rather than its hops.
///
/// Ported from `IbuMethods::recipeLevelIbus`.  This belongs to the recipe, not to any one hop
/// addition, and only SMPH accounts for it at all.
pub fn recipe_level_ibus(original_gravity_sg: f64, post_boil_ph: Option<f64>) -> f64 {
    let points = (original_gravity_sg - 1.0).max(0.0) * 1000.0;
    let ph_factor = (1.7348 * (UNADJUSTED_PH - post_boil_ph.unwrap_or(DEFAULT_POST_BOIL_PH)) / 0.70) + 1.0;
    points * 0.0190 * ph_factor
}

/// Bitterness from one hop addition, by the SMPH model.
///
/// Ported from `smph`.  Three separate things reach the glass: the iso-alpha acids made during
/// the boil, those made while the wort cooled, and the bitterness that was never iso-alpha acid
/// -- oxidised alpha and beta acids and polyphenols.  Each loses a different amount on the way.
pub fn smph(parameters: &Parameters, smph: &SmphParameters) -> crate::ibu::Breakdown {
    let volume_liters = parameters.kettle_volume_at_flameout_liters.unwrap_or(parameters.post_boil_volume_liters);
    let ppm_per_gram_of_hops = 1000.0 / volume_liters;
    let original_gravity_sg = parameters.original_gravity_sg.unwrap_or(parameters.wort_gravity_sg);
    let post_boil_ph = smph.post_boil_ph.unwrap_or(DEFAULT_POST_BOIL_PH);
    let boiling_point_c = boiling_point_c(smph);

    // Reference [17].
    let alpha_acids_ppm =
        HOP_FRESHNESS_FACTOR * parameters.alpha_acid_rating * parameters.hops_grams * ppm_per_gram_of_hops;

    // Wort already carrying alpha acid from an earlier addition dissolves less of this one.
    let volume_at_addition_liters = smph.kettle_volume_at_addition_liters.unwrap_or(volume_liters);
    let added_at_addition_ppm = HOP_FRESHNESS_FACTOR * parameters.alpha_acid_rating * parameters.hops_grams * 1000.0
        / volume_at_addition_liters;
    let already_dissolved_ppm = smph.kettle_dissolved_alpha_acids_ppm.unwrap_or(0.0).max(0.0);
    let loss_factor_hopping_rate = if added_at_addition_ppm > 0.0 {
        (dissolved_after_addition_ppm(already_dissolved_ppm, added_at_addition_ppm) - already_dissolved_ppm)
            / added_at_addition_ppm
    } else {
        1.0
    };

    let mut dissolved_alpha_acids_ppm = alpha_acids_ppm * loss_factor_hopping_rate;
    let mut iaa_ppm = 0.0;
    let mut effective_steep_time_minutes = 0.0;

    if !parameters.post_boil_addition && parameters.time_in_boil_minutes > 0.0 {
        advance(
            &mut dissolved_alpha_acids_ppm,
            &mut iaa_ppm,
            boiling_point_c + 273.15,
            parameters.time_in_boil_minutes,
        );
        effective_steep_time_minutes +=
            parameters.time_in_boil_minutes * relative_utilization(boiling_point_c).clamp(0.0, 1.0);
    }
    let iaa_at_flameout_ppm = iaa_ppm;

    let cool_time_minutes = parameters.cool_time_minutes.unwrap_or(0.0).max(0.0);
    if cool_time_minutes > 0.0 {
        let rate = cooling_rate(parameters);
        let number_of_steps = ((cool_time_minutes / POST_BOIL_STEP_MINUTES).ceil() as i64).max(1);
        let step_size = cool_time_minutes / number_of_steps as f64;
        for step in 0..number_of_steps {
            let mid_step_minutes = parameters.post_boil_delay_minutes + (step as f64 + 0.5) * step_size;
            let temp_c = post_boil_wort_temperature_c(
                mid_step_minutes,
                &parameters.post_boil_temperature_profile,
                rate,
                boiling_point_c,
            );
            // Once the wort is below 50 C nothing further happens, so stop rather than keep
            // integrating a rate that is effectively zero.
            if temp_c < MINIMUM_ISOMERIZATION_TEMP_C {
                break;
            }
            advance(&mut dissolved_alpha_acids_ppm, &mut iaa_ppm, temp_c + 273.15, step_size);
            effective_steep_time_minutes += step_size * relative_utilization(temp_c).clamp(0.0, 1.0);
        }
    }

    let loss_factor_gravity = loss_factor_gravity(original_gravity_sg, effective_steep_time_minutes);
    let loss_factor_age = loss_factor_age(BEER_AGE_WEEKS);
    let loss_factor_wort_clarity = loss_factor_wort_clarity(smph.wort_clarity);
    let loss_factor_krausen_iaa = loss_factor_krausen_iaa(smph.krausen_handling);
    let loss_factor_krausen_non_iaa = loss_factor_krausen_non_iaa(smph.krausen_handling);
    let loss_factor_ferment = LOSS_FACTOR_FERMENT * loss_factor_flocculation(smph.yeast_flocculation);

    let loss_factor_iaa = LOSS_FACTOR_BOIL
        * loss_factor_gravity
        * loss_factor_ph_iaa(post_boil_ph)
        * loss_factor_wort_clarity
        * loss_factor_ferment
        * loss_factor_krausen_iaa
        * loss_factor_age;
    let loss_factor_oaa = LOSS_FACTOR_BOIL
        * loss_factor_gravity
        * loss_factor_ph_oaa(post_boil_ph)
        * loss_factor_ferment
        * loss_factor_krausen_non_iaa
        * loss_factor_age;
    let loss_factor_oba = loss_factor_ferment * loss_factor_krausen_non_iaa * loss_factor_age;
    let loss_factor_pp_hops = LOSS_FACTOR_PP_PRECIPITATE * LOSS_FACTOR_PP_FERMENT * loss_factor_krausen_non_iaa;

    let pellets = matches!(parameters.hop_form, HopForm::Pellet | HopForm::Powder);
    // Oxidation is whichever is greater: what happened before the kettle, or what happens in it.
    let oaa = parameters.alpha_acid_rating
        * pre_kettle_alpha_acid_oxidation(smph.six_month_alpha_loss).max(
            HOP_FRESHNESS_FACTOR
                * if pellets { PELLET_OAA_FACTOR } else { 1.0 }
                * loss_factor_hopping_rate
                * OAA_BOIL_FACTOR,
        );
    let oaa_beer_ppm = oaa * parameters.hops_grams * ppm_per_gram_of_hops * loss_factor_oaa;

    let oba = OBA_BOIL_FACTOR * smph.beta_acid_rating.unwrap_or(0.0) * (1.0 - HOP_FRESHNESS_FACTOR);
    let oba_beer_ppm = oba * parameters.hops_grams * ppm_per_gram_of_hops * loss_factor_oba;

    let pp_hops_beer_ppm = smph.polyphenol_rating.unwrap_or(DEFAULT_POLYPHENOL_RATING)
        * parameters.hops_grams
        * ppm_per_gram_of_hops
        * loss_factor_pp_hops;

    crate::ibu::Breakdown {
        boil_ibus: IBU_PER_IAA_PPM * iaa_at_flameout_ppm * loss_factor_iaa,
        post_boil_ibus: IBU_PER_IAA_PPM * (iaa_ppm - iaa_at_flameout_ppm) * loss_factor_iaa,
        non_iaa_ibus: IBU_PER_IAA_PPM
            * (oaa_beer_ppm * SCALE_OAA + oba_beer_ppm * SCALE_OBA + pp_hops_beer_ppm * SCALE_PP_HOPS),
    }
}
