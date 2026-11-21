// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-measure/src/numeric.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Polynomial evaluation, root finding and table interpolation.
//!
//! Ported from `Polynomial` and `interpolatedConversion` in `src/calculations/Algorithms.cpp`.
//! They live here rather than in `bt-calc` because the Plato and Brix units need them, and
//! `bt-calc` is the crate that depends on this one, not the other way round.

/// `ROOT_PRECISION` in `src/calculations/Algorithms.cpp`.
const ROOT_PRECISION: f64 = 0.0000001;

/// A polynomial, lowest-order coefficient first.
#[derive(Debug, Clone, PartialEq)]
pub struct Polynomial {
    coefficients: Vec<f64>,
}

impl Polynomial {
    pub fn new(coefficients: impl Into<Vec<f64>>) -> Self {
        Self { coefficients: coefficients.into() }
    }

    /// Evaluate at `x`.
    ///
    /// Summed from the highest order down and with the constant term added last, as the C++ does:
    /// floating-point addition is not associative, so the order is part of the answer.
    pub fn eval(&self, x: f64) -> f64 {
        let mut result = 0.0;
        for order in (1..self.coefficients.len()).rev() {
            result += self.coefficients[order] * x.powi(order as i32);
        }
        result + self.coefficients.first().copied().unwrap_or(0.0)
    }

    /// Subtract a constant, which turns "where does this polynomial equal y" into a root search.
    pub fn minus_constant(&self, constant: f64) -> Self {
        let mut coefficients = self.coefficients.clone();
        if coefficients.is_empty() {
            coefficients.push(-constant);
        } else {
            coefficients[0] -= constant;
        }
        Self { coefficients }
    }

    /// Find a root between two guesses by the secant method.
    ///
    /// Returns [`f64::INFINITY`] when the guesses run away from each other, matching the C++'s
    /// `HUGE_VAL`.
    pub fn root_find(&self, x0: f64, x1: f64) -> f64 {
        let mut guesses = [x0, x1];
        let mut new_guess = x0;
        let max_allowable_separation = (x0 - x1).abs() * 1e3;

        while (guesses[0] - guesses[1]).abs() > ROOT_PRECISION {
            let (first, second) = (self.eval(guesses[0]), self.eval(guesses[1]));
            new_guess = guesses[1] - (guesses[1] - guesses[0]) * second / (second - first);
            guesses = [guesses[1], new_guess];
            if (guesses[0] - guesses[1]).abs() > max_allowable_separation {
                return f64::INFINITY;
            }
        }
        new_guess
    }
}

/// Convert by interpolating between observed data points.
///
/// `table` must be sorted ascending on the `from` column.  An exact match returns its `to` value;
/// otherwise the result is a linear interpolation between the neighbouring rows.  A value outside
/// the table clamps to the nearest end, which is what the C++ does after logging a warning.
///
/// Ported from `interpolatedConversion` in `src/calculations/Algorithms.cpp`.
pub fn interpolate(table: &[[f64; 3]], value: f64, from: usize, to: usize) -> f64 {
    let Some(first) = table.first() else {
        return 0.0;
    };
    let last = table.last().expect("table is not empty");

    // The first row whose `from` is not less than the value we are looking for.
    let index = table.partition_point(|row| row[from] < value);
    if index == table.len() {
        return last[to]; // too large to convert
    }
    let larger = &table[index];
    if larger[from] == value {
        return larger[to];
    }
    if index == 0 {
        return first[to]; // too small to convert
    }

    let smaller = &table[index - 1];
    let position_in_range = (value - smaller[from]) / (larger[from] - smaller[from]);
    position_in_range * (larger[to] - smaller[to]) + smaller[to]
}
