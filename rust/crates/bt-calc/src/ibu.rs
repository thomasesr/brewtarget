// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/ibu.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! International bitterness units.
//!
//! Ported from `src/calculations/IbuMethods.cpp` and `src/model/Ibu.h`.
//!
//! Seven published methods disagree about how much of a hop's alpha acid ends up in the glass,
//! and brewers disagree about which to believe, so all seven are here and the choice is a
//! parameter.  The formulae are transcribed as written, lookup tables and all.

use bt_measure::Unit;

/// Which published method to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Formula {
    #[default]
    Tinseth,
    Rager,
    Noonan,
    Garetz,
    Daniels,
    /// Tinseth with a correction for the bitterness gained while the wort cools.
    MIbu,
    /// Hosom's "SMPH" model, which accounts for losses after the boil as well.
    Smph,
}

/// The form a hop was added in, which changes how much of its alpha acid is extracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HopForm {
    #[default]
    Whole,
    Plug,
    Pellet,
    Powder,
    Extract,
}

/// Where the bitterness came from.
///
/// Ported from `Ibu::Breakdown`.  Keeping the parts separate is what lets the recipe screen show
/// how much of a beer's bitterness arrives after flameout.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Breakdown {
    pub boil_ibus: f64,
    pub post_boil_ibus: f64,
    /// Bitterness from something other than isomerised alpha acid, which only SMPH models.
    pub non_iaa_ibus: f64,
}

impl Breakdown {
    pub fn total(&self) -> f64 {
        self.boil_ibus + self.post_boil_ibus + self.non_iaa_ibus
    }

    fn scaled(self, factor: f64) -> Self {
        Self {
            boil_ibus: self.boil_ibus * factor,
            post_boil_ibus: self.post_boil_ibus * factor,
            non_iaa_ibus: self.non_iaa_ibus * factor,
        }
    }
}

/// Everything one hop addition contributes.
///
/// Ported from `IbuMethods::IbuCalculationParms`.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameters {
    /// Alpha acid as a fraction, not a percentage.
    pub alpha_acid_rating: f64,
    pub hops_grams: f64,
    pub post_boil_volume_liters: f64,
    pub wort_gravity_sg: f64,
    pub time_in_boil_minutes: f64,
    pub hop_form: HopForm,
    /// True for a hop added after flameout, which the boil-only methods ignore entirely.
    pub post_boil_addition: bool,
    pub post_boil_delay_minutes: f64,
    pub pre_boil_volume_liters: Option<f64>,
    pub kettle_volume_at_flameout_liters: Option<f64>,
    pub original_gravity_sg: Option<f64>,
    /// Garetz alone feeds its own answer back in; see [`garetz_hopping_rate_factor`].
    pub hopping_rate_factor: f64,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            alpha_acid_rating: 0.0,
            hops_grams: 0.0,
            post_boil_volume_liters: 1.0,
            wort_gravity_sg: 1.0,
            time_in_boil_minutes: 0.0,
            hop_form: HopForm::default(),
            post_boil_addition: false,
            post_boil_delay_minutes: 0.0,
            pre_boil_volume_liters: None,
            kettle_volume_at_flameout_liters: None,
            original_gravity_sg: None,
            hopping_rate_factor: 1.0,
        }
    }
}

/// Tinseth's alpha acid utilisation: how much of the alpha acid isomerises.
fn decimal_alpha_acid_utilization(wort_gravity_sg: f64, time_in_boil_minutes: f64) -> f64 {
    let boil_time_factor = (1.0 - (-0.04 * time_in_boil_minutes).exp()) / 4.15;
    let bigness_factor = 1.65 * 0.000125f64.powf(wort_gravity_sg - 1.0);
    bigness_factor * boil_time_factor
}

/// Walk a table of (upper bound, value) pairs, taking the first entry the time falls within.
///
/// The C++ leaves the last value in place when nothing matches, so a time past the end of the
/// table takes the final entry; reproduced here.
fn look_up_utilization_pct(table: &[(f64, f64)], time_in_boil_minutes: f64) -> f64 {
    let mut utilization_pct = 0.0;
    for &(upper_bound_minutes, entry) in table {
        utilization_pct = entry;
        if time_in_boil_minutes <= upper_bound_minutes {
            break;
        }
    }
    utilization_pct
}

/// Rager's correction for gravity: above 1.050 the wort extracts less.
fn rager_gravity_correction(boil_gravity_sg: f64) -> f64 {
    if boil_gravity_sg > 1.050 { 1.0 + (boil_gravity_sg - 1.050) / 0.2 } else { 1.0 }
}

fn is_pellet_or_powder(parameters: &Parameters) -> bool {
    matches!(parameters.hop_form, HopForm::Pellet | HopForm::Powder)
}

/// The adjustment for hop form used by the methods that do not model it themselves.
fn generic_hop_form_factor(form: HopForm) -> f64 {
    match form {
        HopForm::Whole | HopForm::Extract => 1.0,
        HopForm::Plug => 1.02,
        HopForm::Pellet | HopForm::Powder => 1.10,
    }
}

fn tinseth(parameters: &Parameters) -> Breakdown {
    if parameters.post_boil_addition {
        return Breakdown::default();
    }
    let mg_per_liter_of_added_alpha_acids =
        (parameters.alpha_acid_rating * parameters.hops_grams * 1000.0) / parameters.post_boil_volume_liters;
    let utilization = decimal_alpha_acid_utilization(parameters.wort_gravity_sg, parameters.time_in_boil_minutes);
    Breakdown { boil_ibus: utilization * mg_per_liter_of_added_alpha_acids, ..Default::default() }
}

fn rager(parameters: &Parameters) -> Breakdown {
    if parameters.post_boil_addition {
        return Breakdown::default();
    }
    let utilization = (18.11 + 13.86 * ((parameters.time_in_boil_minutes - 31.32) / 18.17).tanh()) / 100.0;
    // Note this is not rager_gravity_correction: here the factor is zero below 1.050, not one,
    // because it is added to one rather than multiplied in.
    let gravity_factor =
        if parameters.wort_gravity_sg > 1.050 { (parameters.wort_gravity_sg - 1.050) / 0.2 } else { 0.0 };
    Breakdown {
        boil_ibus: (parameters.hops_grams * utilization * parameters.alpha_acid_rating * 1000.0)
            / (parameters.post_boil_volume_liters * (1.0 + gravity_factor)),
        ..Default::default()
    }
}

fn noonan(parameters: &Parameters) -> Breakdown {
    if parameters.post_boil_addition {
        return Breakdown::default();
    }
    let us_gallons = Unit::by_name("us_gallons").expect("the US gallon");
    let ounces = Unit::by_name("ounces").expect("the ounce");
    let volume_factor = us_gallons.to_canonical(5.0).quantity / parameters.post_boil_volume_liters;
    let hops_factor = parameters.hops_grams / (ounces.to_canonical(1.0).quantity * 1000.0);
    let utilization_over_time = bt_measure::Polynomial::new(vec![
        0.7000029428,
        -0.08868853463,
        0.02720809386,
        -0.002340415323,
        0.00009925450081,
        -0.000002102006144,
        0.00000002132644293,
        -0.00000000008229488217,
    ]);
    let utilization_factor = match parameters.wort_gravity_sg {
        sg if sg <= 1.050 => 1.0,
        sg if sg <= 1.065 => 0.9286,
        sg if sg <= 1.085 => 0.8571,
        _ => 0.75,
    };
    Breakdown {
        boil_ibus: volume_factor
            * (hops_factor
                * (100.0 * parameters.alpha_acid_rating)
                * utilization_over_time.eval(parameters.time_in_boil_minutes))
            * utilization_factor,
        ..Default::default()
    }
}

fn garetz(parameters: &Parameters) -> Breakdown {
    if parameters.post_boil_addition {
        return Breakdown::default();
    }
    let mut utilization_pct = look_up_utilization_pct(
        &[
            (10.0, 0.0),
            (15.0, 2.0),
            (20.0, 5.0),
            (25.0, 8.0),
            (30.0, 11.0),
            (35.0, 14.0),
            (40.0, 16.0),
            (45.0, 18.0),
            (50.0, 19.0),
            (60.0, 20.0),
            (70.0, 21.0),
            (80.0, 22.0),
            (90.0, 23.0),
        ],
        parameters.time_in_boil_minutes,
    );
    if is_pellet_or_powder(parameters) && parameters.time_in_boil_minutes <= 30.0 {
        utilization_pct *= 1.10;
    }

    let final_volume_liters = parameters.kettle_volume_at_flameout_liters.unwrap_or(parameters.post_boil_volume_liters);
    let concentration_factor = match parameters.pre_boil_volume_liters {
        Some(pre_boil) if pre_boil > 0.0 => final_volume_liters / pre_boil,
        _ => 1.0,
    };
    let original_gravity_sg = parameters.original_gravity_sg.unwrap_or(parameters.wort_gravity_sg);
    let boil_gravity_sg = (concentration_factor * (original_gravity_sg - 1.0)) + 1.0;
    let gravity_factor = rager_gravity_correction(boil_gravity_sg);
    let temperature_factor = 1.0;
    let combined_adjustments = gravity_factor * parameters.hopping_rate_factor * temperature_factor;

    Breakdown {
        boil_ibus: (utilization_pct / 100.0) * parameters.alpha_acid_rating * parameters.hops_grams * 1000.0
            / (parameters.post_boil_volume_liters * combined_adjustments),
        ..Default::default()
    }
}

fn daniels(parameters: &Parameters) -> Breakdown {
    if parameters.post_boil_addition {
        return Breakdown::default();
    }
    // The bounds are a whisker below the round numbers so that a 10-minute addition takes the
    // next band up, which is how the published table reads.
    const JUST_UNDER: f64 = 1.0e-9;
    let pellets = [
        (10.0 - JUST_UNDER, 6.0),
        (20.0 - JUST_UNDER, 15.0),
        (30.0 - JUST_UNDER, 19.0),
        (45.0 - JUST_UNDER, 24.0),
        (60.0 - JUST_UNDER, 27.0),
        (75.0 - JUST_UNDER, 30.0),
        (f64::MAX, 34.0),
    ];
    let whole = [
        (10.0 - JUST_UNDER, 5.0),
        (20.0 - JUST_UNDER, 12.0),
        (30.0 - JUST_UNDER, 15.0),
        (45.0 - JUST_UNDER, 19.0),
        (60.0 - JUST_UNDER, 22.0),
        (75.0 - JUST_UNDER, 24.0),
        (f64::MAX, 27.0),
    ];
    let table: &[(f64, f64)] = if is_pellet_or_powder(parameters) { &pellets } else { &whole };
    let utilization_pct = look_up_utilization_pct(table, parameters.time_in_boil_minutes);
    let gravity_correction = rager_gravity_correction(parameters.wort_gravity_sg);

    Breakdown {
        boil_ibus: (utilization_pct / 100.0) * parameters.alpha_acid_rating * parameters.hops_grams * 1000.0
            / (parameters.post_boil_volume_liters * gravity_correction * parameters.hopping_rate_factor),
        ..Default::default()
    }
}

/// Garetz's hopping rate factor, which depends on the answer it is used to compute.
///
/// `HF = 1 + a × IBU` where `a = CF / 260` and `IBU = U / HF`, so `HF² - HF - a × U = 0` and the
/// positive root is the factor.  Ported from `IbuMethods::garetzHoppingRateFactor`.
pub fn garetz_hopping_rate_factor(total_ibus_before_hopping_rate_factor: f64, concentration_factor: f64) -> f64 {
    let a_times_u = (concentration_factor / 260.0 * total_ibus_before_hopping_rate_factor).max(0.0);
    (1.0 + (1.0 + 4.0 * a_times_u).sqrt()) / 2.0
}

/// Whether a method models hop form itself, or needs the generic adjustment applying afterwards.
fn handles_hop_form(formula: Formula) -> bool {
    matches!(formula, Formula::Garetz | Formula::Daniels | Formula::Smph)
}

/// Whether a method accounts for bitterness gained after flameout, while the wort cools.
pub fn models_post_boil_isomerization(formula: Formula) -> bool {
    matches!(formula, Formula::MIbu | Formula::Smph)
}

/// Whether a method feeds its own answer back in through a hopping rate factor.
pub fn uses_hopping_rate_factor(formula: Formula) -> bool {
    matches!(formula, Formula::Garetz | Formula::Daniels)
}

/// Bitterness from one hop addition, broken down by where it came from.
///
/// Ported from `IbuMethods::getIbuBreakdown`.
pub fn ibu_breakdown(parameters: &Parameters, formula: Formula) -> Breakdown {
    let result = match formula {
        Formula::Tinseth => tinseth(parameters),
        Formula::Rager => rager(parameters),
        Formula::Noonan => noonan(parameters),
        Formula::Garetz => garetz(parameters),
        Formula::Daniels => daniels(parameters),
        Formula::MIbu | Formula::Smph => {
            // Both model what happens after flameout and need the cooling curve, the kettle
            // geometry and the fermentation losses.  Not yet ported; see PLAN.md Phase 5.
            unimplemented!("{formula:?} is not ported yet")
        }
    };
    if handles_hop_form(formula) { result } else { result.scaled(generic_hop_form_factor(parameters.hop_form)) }
}

/// Total bitterness from one hop addition.
pub fn ibus(parameters: &Parameters, formula: Formula) -> f64 {
    ibu_breakdown(parameters, formula).total()
}
