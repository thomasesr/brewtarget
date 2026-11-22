// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/src/views/format.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Turning stored values into something to read.
//!
//! This is the presentation edge: values are held canonically in the database and converted here
//! and nowhere else, exactly as `src/measurement/` does it.

use bt_measure::Unit;
use bt_model::enums::UnknownValue;

/// A column with nothing in it.
///
/// An en dash rather than an empty cell, so that "not recorded" is visibly different from a
/// value that happens to be blank.
pub const NOTHING: &str = "–";

/// Text, or a dash if there is none.
pub fn text(value: Option<&str>) -> String {
    match value.map(str::trim) {
        Some(text) if !text.is_empty() => text.to_owned(),
        _ => NOTHING.to_owned(),
    }
}

/// A number to a fixed number of decimal places.
pub fn number(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => NOTHING.to_owned(),
    }
}

/// A percentage, as the Qt tables show them: one decimal place and a sign.
pub fn percent(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.1}%"),
        None => NOTHING.to_owned(),
    }
}

/// An enumerated value, as a word rather than as it is stored.
///
/// The stored strings are lower case and underscored because that is what goes in the column.
/// The C++ has a second mapping of its own for display (`EnumDisplayNameMapping`, which is also
/// where translation happens); until that is extracted, this tidies the stored form, which is
/// close enough to read and never wrong about *which* value it is.
pub fn label<T>(value: Result<Option<T>, UnknownValue>, as_str: impl Fn(&T) -> &'static str) -> String {
    match value {
        Ok(Some(value)) => title_case(as_str(&value)),
        Ok(None) => NOTHING.to_owned(),
        // A value this build does not recognise is shown rather than hidden: it is in the
        // database, and a user looking for it should be able to see that it is there.
        Err(UnknownValue { value, .. }) => format!("{value} (unrecognised)"),
    }
}

/// `drain mash tun` becomes `Drain Mash Tun`.
fn title_case(value: &str) -> String {
    value
        .split(['_', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A malt's colour in SRM, which is what the catalogue shows, from the degrees Lovibond the
/// column holds.
///
/// Ported from `Fermentable::color_srm`: SRM is the canonical colour unit, so this is the
/// ordinary unit conversion rather than a formula of its own.
pub fn color_srm(lovibond: Option<f64>) -> String {
    let Some(lovibond) = lovibond else { return NOTHING.to_owned() };
    match Unit::by_name("lovibond") {
        Some(unit) => format!("{:.1}", unit.to_canonical(lovibond).quantity),
        None => NOTHING.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use bt_model::enums::FermentableType;

    use super::*;

    #[test]
    fn nothing_recorded_reads_differently_from_nothing_written() {
        assert_eq!(text(None), NOTHING);
        assert_eq!(text(Some("")), NOTHING, "a blank string is as good as nothing recorded");
        assert_eq!(text(Some("  ")), NOTHING);
        assert_eq!(text(Some(" Maris Otter ")), "Maris Otter");
        assert_eq!(number(None, 1), NOTHING);
        assert_eq!(percent(None), NOTHING);
    }

    #[test]
    fn numbers_are_shown_to_the_places_the_qt_tables_show_them() {
        assert_eq!(number(Some(18.9271), 1), "18.9");
        assert_eq!(number(Some(5.0), 2), "5.00");
        assert_eq!(percent(Some(81.52)), "81.5%");
    }

    /// Ported from `Fermentable::color_srm`, which converts through the canonical colour unit
    /// rather than with a formula of its own.
    #[test]
    fn colour_is_shown_in_srm_though_it_is_stored_in_lovibond() {
        // The conversion in `Unit::lovibond` is 1.3546 L - 0.76.
        assert_eq!(color_srm(Some(1.0)), "0.6");
        assert_eq!(color_srm(Some(3.0)), "3.3");
        assert_eq!(color_srm(Some(40.0)), "53.4");
        assert_eq!(color_srm(None), NOTHING);
    }

    #[test]
    fn stored_words_are_tidied_without_changing_which_value_they_are() {
        let grain: Result<Option<FermentableType>, _> = Ok(Some(FermentableType::Grain));
        assert_eq!(label(grain, |value| value.as_str()), "Grain");

        let none: Result<Option<FermentableType>, _> = Ok(None);
        assert_eq!(label(none, |value| value.as_str()), NOTHING);
    }

    /// A value this build does not know is shown, not hidden: it is in the database, and
    /// someone wondering where their row went should be able to see that it is there.
    #[test]
    fn an_unrecognised_value_is_shown_rather_than_swallowed() {
        let unknown: Result<Option<FermentableType>, _> =
            Err(UnknownValue { enumeration: "FermentableType", value: "unobtainium".to_owned() });
        assert_eq!(label(unknown, |value: &FermentableType| value.as_str()), "unobtainium (unrecognised)");
    }
}
