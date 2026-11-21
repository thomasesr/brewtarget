// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-calc/src/algorithms.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Ported from `src/calculations/Algorithms.cpp`.
//!
//! Transcribed literally: the same variable names, the same formula shape, the same magic
//! constants, in the same order.  These are empirical fits, and rearranging one to look tidier
//! changes the answer in the last decimal places.
//!
//! The Plato, Brix and polynomial machinery lives in `bt-measure`, because the units need it and
//! this crate is the one that depends on that one.  It is re-exported here so that the C++ names
//! all resolve in one place.

use bt_measure::Polynomial;
use bt_measure::unit_data;

pub use bt_measure::formula::{brix_to_sg_at_20c, plato_to_sg_20c20c, sg_20c20c_to_plato, sg_at_20c_to_brix};

use crate::data::{calculation_data, constant};

/// Round to `precision` decimal places.
pub fn round(value: f64, precision: i32) -> f64 {
    let power_of_ten = 10f64.powi(precision);
    (value * power_of_ten).round() / power_of_ten
}

/// The additive correction for a hydrometer calibrated at 15 °C but read at another temperature.
pub fn hydrometer_15c_correction(celsius: f64) -> f64 {
    Polynomial::new(unit_data().polynomials["hydroCorrection15CPoly"].clone()).eval(celsius) * 1e-3
}

/// Water density in kg/L at a given temperature.
pub fn water_density_kg_l(celsius: f64) -> f64 {
    Polynomial::new(unit_data().polynomials["waterDensityPoly_C"].clone()).eval(celsius)
}

/// Plato from dissolved sucrose and the volume of wort it is dissolved in.
///
/// Assumes the volume of the sucrose and the volume of the water add up to the volume of the
/// wort.  Zero sugar in zero wort would divide by zero, so it returns the Plato of water instead.
pub fn plato_from_sugar_mass_and_wort_volume(sugar_kg: f64, wort_l: f64) -> f64 {
    let water_kg = wort_l - sugar_kg / constant("sucroseDensity_kgL");
    let total_mass_kg = sugar_kg + water_kg;
    if total_mass_kg == 0.0 {
        return 0.0;
    }
    sugar_kg / total_mass_kg * 100.0
}

/// Refractive index of a solution of the given Plato.
pub fn refractive_index(plato: f64) -> f64 {
    1.33302 + 0.001427193 * plato + 0.000005791157 * plato * plato
}

/// Alcohol by volume from specific gravity and Plato, by the refractometer method.
pub fn abv_by_sg_plato(sg: f64, plato: f64) -> f64 {
    (277.8851 - 277.4 * sg + 0.9956 * plato + 0.00523 * plato * plato + 0.000013 * plato * plato * plato) * (sg / 0.79)
}

/// Alcohol by weight from specific gravity and Plato.
pub fn abw_by_sg_plato(sg: f64, plato: f64) -> f64 {
    let ri = refractive_index(plato);
    1017.5596 - 277.4 * sg + ri * (937.8135 * ri - 1805.1228)
}

/// Specific gravity from a starting and a current Plato reading.
pub fn sg_by_starting_plato(starting_plato: f64, current_plato: f64) -> f64 {
    let sp2 = starting_plato * starting_plato;
    let sp3 = sp2 * starting_plato;
    let cp2 = current_plato * current_plato;
    let cp3 = cp2 * current_plato;
    1.001843 - 0.002318474 * starting_plato - 0.000007775 * sp2 - 0.000000034 * sp3
        + 0.00574 * current_plato
        + 0.00003344 * cp2
        + 0.000000086 * cp3
}

/// Real extract from specific gravity and Plato.
pub fn real_extract(sg: f64, plato: f64) -> f64 {
    let ri = refractive_index(plato);
    194.5935 + 129.8 * sg + ri * (410.8815 * ri - 790.8732)
}

/// Excess gravity: the thousandths by which a gravity exceeds that of water.
fn specific_gravity_to_excess_gravity(sg: f64) -> f64 {
    (sg - 1.0) * 1000.0
}

/// Alcohol by volume from original and final gravity.
///
/// Uses the method the UK Laboratory of the Government Chemist publishes, which HM Revenue and
/// Customs tell microbreweries with "no or minimal laboratory facilities" to use: look the drop
/// in gravity up in a table and multiply by the factor for that band.
///
/// Falls back to the older formula when the drop is off the end of the table, which is what the
/// C++ does after logging.  A result outside the band's expected range is likewise only worth a
/// warning -- the table is a published approximation, not a law of nature.
///
/// # Panics
///
/// If `fg` is above `og`, which means the arguments were passed the wrong way round.  The C++
/// asserts the same thing.
pub fn abv_from_og_and_fg(og: f64, fg: f64) -> f64 {
    assert!(og >= fg, "final gravity {fg} is above original gravity {og}");

    let excess_gravity_diff_x10 =
        (10.0 * (specific_gravity_to_excess_gravity(og) - specific_gravity_to_excess_gravity(fg))).round();
    let excess_gravity_diff = excess_gravity_diff_x10 / 10.0;

    let by_fallback_method = (76.08 * (og - fg) / (1.775 - og)) * (fg / 0.794);
    let Some(row) = calculation_data().gravity_difference_factors.iter().find(|row| {
        row.excess_gravity_diff_x10_min <= excess_gravity_diff_x10
            && excess_gravity_diff_x10 <= row.excess_gravity_diff_x10_max
    }) else {
        tracing::warn!(excess_gravity_diff, "no HMRC gravity factor for this difference; using the older formula");
        return by_fallback_method;
    };

    let by_hmrc_method = excess_gravity_diff * row.factor;
    if by_hmrc_method < row.pct_abv_min || by_hmrc_method > row.pct_abv_max {
        tracing::warn!(
            abv = by_hmrc_method,
            expected_min = row.pct_abv_min,
            expected_max = row.pct_abv_max,
            "calculated ABV is outside the range the table expects"
        );
    }
    by_hmrc_method
}

/// Correct a hydrometer reading taken at one temperature to what it would read at another.
///
/// The polynomial is in Fahrenheit, so both temperatures are converted first -- as the C++ does,
/// via the Fahrenheit unit rather than by inlining the conversion.
pub fn correct_sg_for_temperature(measured_gravity_sg: f64, reading_temp_c: f64, calibration_temp_c: f64) -> f64 {
    let fahrenheit = bt_measure::Unit::by_name("fahrenheit").expect("the Fahrenheit unit");
    let tr = fahrenheit.from_canonical(reading_temp_c);
    let tc = fahrenheit.from_canonical(calibration_temp_c);
    let correction =
        |t: f64| 1.00130346 - 0.000134722124 * t + 0.00000204052596 * t * t - 0.00000000232820948 * t * t * t;
    measured_gravity_sg * (correction(tr) / correction(tc))
}
