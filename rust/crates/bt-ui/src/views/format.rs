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

/// An enumerated value, by the name it is shown by.
///
/// What is stored and what is shown are two different strings: the database holds
/// `drain mash tun` and the application shows "Batch Sparge (Drain Mash Tun)".  Neither follows
/// from the other, so `display_name` is extracted from the C++ rather than derived -- see
/// `tools/extract-labels.py`.
pub fn label<T>(value: Result<Option<T>, UnknownValue>, display_name: impl Fn(&T) -> &'static str) -> String {
    match value {
        Ok(Some(value)) => display_name(&value).to_owned(),
        Ok(None) => NOTHING.to_owned(),
        // A value this build does not recognise is shown rather than hidden: it is in the
        // database, and a user looking for it should be able to see that it is there.
        Err(UnknownValue { value, .. }) => format!("{value} (unrecognised)"),
    }
}

/// What a property is called when it is shown.
///
/// For the things the interface shows that no column holds -- the inventory a catalogue ends
/// with is worked out from the stock purchases, not stored.
pub fn property_label(property: &'static str) -> &'static str {
    bt_model::tables::label_for_property(property).unwrap_or(property)
}

/// What a column is called when it is shown.
///
/// Falls back to the column name, which is at least true, for the few columns the C++ never
/// shows and therefore never named.
pub fn column_label(table: &str, column: &'static str) -> &'static str {
    bt_model::tables::label_for(table, column).unwrap_or(column)
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
    use bt_model::enums::{FermentableType, MashStepType};

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

    /// What is stored and what is shown are different strings, and the one shown is the one
    /// the C++ shows -- not a tidied-up version of the stored form, which would be a guess.
    #[test]
    fn an_enumerated_value_is_shown_by_the_name_the_application_gives_it() {
        let dry: Result<Option<FermentableType>, _> = Ok(Some(FermentableType::DryExtract));
        assert_eq!(label(dry, FermentableType::display_name), "Dry Extract", "stored as `dry extract`");

        // The one that makes the point: tidying the stored string would read "Drain Mash Tun".
        let sparge: Result<Option<MashStepType>, _> = Ok(Some(MashStepType::BatchSparge));
        assert_eq!(label(sparge, MashStepType::display_name), "Batch Sparge (Drain Mash Tun)");
        assert_eq!(MashStepType::BatchSparge.as_str(), "drain mash tun", "which is not what is stored");

        let none: Result<Option<FermentableType>, _> = Ok(None);
        assert_eq!(label(none, FermentableType::display_name), NOTHING);
    }

    /// Column labels come from the C++ too, for the same reason.
    #[test]
    fn a_column_is_shown_by_the_name_the_application_gives_it() {
        assert_eq!(column_label("fermentable", "fine_grind_yield_pct"), "Yield (DBFG) %");
        assert_eq!(column_label("fermentable", "color_lovibond"), "Color");
        assert_eq!(column_label("hop", "alpha_pct"), "% Alpha");
        // A column the C++ never shows falls back to its own name rather than to nothing.
        assert_eq!(column_label("hop", "no_such_column"), "no_such_column");
    }

    /// A value this build does not know is shown, not hidden: it is in the database, and
    /// someone wondering where their row went should be able to see that it is there.
    #[test]
    fn an_unrecognised_value_is_shown_rather_than_swallowed() {
        let unknown: Result<Option<FermentableType>, _> =
            Err(UnknownValue { enumeration: "FermentableType", value: "unobtainium".to_owned() });
        assert_eq!(label(unknown, FermentableType::display_name), "unobtainium (unrecognised)");
    }
}
