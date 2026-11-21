// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-measure/tests/measurement.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Mirrors `src/unitTests/MeasurementTests.cpp`, with the same values and tolerances.
//!
//! `testUnitInverses` in the C++ walks a hand-maintained list of units, carrying the warning "If
//! you add a new Unit, please add it here!".  Here every unit in `schema/units.json` is covered,
//! so a new one cannot be forgotten.

use bt_measure::{Amount, Unit, UnitSystem, unit_data};

/// `UnitTests::fuzzyComp`: equal to within `tolerance`.
fn fuzzy(actual: f64, expected: f64, tolerance: f64) -> bool {
    (actual - expected).abs() <= tolerance
}

fn system(name: &str) -> UnitSystem {
    UnitSystem::by_name(name).unwrap_or_else(|| panic!("unit system {name}"))
}

fn unit(name: &str) -> Unit {
    Unit::by_name(name).unwrap_or_else(|| panic!("unit {name}"))
}

#[test]
fn unit_data_loads() {
    let data = unit_data();
    assert_eq!(data.units.len(), 79);
    assert_eq!(data.unit_systems.len(), 40);
    assert_eq!(data.physical_quantities.len(), 18);
    assert_eq!(data.sucrose_conversions.len(), 801);
}

/// Mirrors `MeasurementTests::testUnitConversions`.
#[test]
fn unit_conversions() {
    // "5.500 gal" in a US Customary volume field, where the default unit is litres: the
    // abbreviation wins, so this is 5.5 US gallons.
    let converted = system("volume_UsCustomary").string_to_si("5.500 gal", unit("liters"));
    assert!(fuzzy(converted.quantity, 20.820, 0.001), "US gallons to litres v1: {}", converted.quantity);

    // "5.500" with no unit named, in a field whose default unit is US gallons.
    let converted = system("volume_UsCustomary").string_to_si("5.500", unit("us_gallons"));
    assert!(fuzzy(converted.quantity, 20.820, 0.001), "US gallons to litres v2: {}", converted.quantity);

    // "9.994 P" read as Plato, against specific gravity.
    let converted = system("density_Plato").string_to_si("9.994 P", unit("specificGravity"));
    assert!(fuzzy(converted.quantity, 1.040, 0.001), "Plato to SG: {}", converted.quantity);

    // "1,083 ebc" -- the comma is a digit group separator, so this is 1083 EBC.
    let converted = system("color_StandardReferenceMethod").string_to_si("1,083 ebc", unit("srm"));
    assert!(fuzzy(converted.quantity, 550.0, 1.0), "EBC to SRM: {}", converted.quantity);
}

/// The ambiguous case from `testUnitConversions`: "gal" alone could be either gallon.
///
/// 5.5 US gallons are 20.820 litres and 5.5 imperial gallons are 25.004, and with no other
/// context the C++ accepts either reading.
#[test]
fn an_unqualified_gallon_may_be_either_kind() {
    for system_name in ["volume_UsCustomary", "volume_Imperial"] {
        let converted = system(system_name).string_to_si("5.500 gal", unit("liters"));
        assert!(
            fuzzy(converted.quantity, 20.820, 0.001) || fuzzy(converted.quantity, 25.004, 0.001),
            "{system_name}: {}",
            converted.quantity
        );
    }
}

/// Mirrors `MeasurementTests::testUnitInverses`, over every unit rather than a hand-kept list.
#[test]
fn every_unit_is_its_own_inverse() {
    for unit in Unit::all() {
        for value in [1.05, 5.0, 12.3, 25.0] {
            let canonical = unit.to_canonical(value);
            let round_trip = unit.from_canonical(canonical.quantity);
            assert!(
                fuzzy(round_trip, value, value * 1e-6),
                "{}: {value} -> {} (canonical) -> {round_trip}",
                unit.name(),
                canonical.quantity
            );
        }
    }
}

#[test]
fn canonical_units_are_consistent() {
    for unit in Unit::all() {
        let canonical = unit.canonical();
        assert!(canonical.is_canonical(), "{} converts through non-canonical {}", unit.name(), canonical.name());
        assert_eq!(
            canonical.physical_quantity(),
            unit.physical_quantity(),
            "{} converts to a unit measuring something else",
            unit.name()
        );
        // A canonical unit converts to itself unchanged.
        let Amount { quantity, unit: resulting } = canonical.to_canonical(3.5);
        assert_eq!(resulting, canonical);
        assert!(fuzzy(quantity, 3.5, 1e-12), "{} is canonical but scales", canonical.name());
    }
    // Every physical quantity has exactly one canonical unit.
    for quantity in &unit_data().physical_quantities {
        let canonical: Vec<_> =
            Unit::all().filter(|u| u.is_canonical() && u.physical_quantity() == quantity).map(|u| u.name()).collect();
        assert_eq!(canonical.len(), 1, "{quantity} has canonical units {canonical:?}");
    }
}

/// Mirrors `MeasurementTests::testAlgorithms`.
///
/// The expected values come from `testData/expected/sg-brix-equivalence.json`, lifted from the
/// C++ test by `tools/extract-test-data.py`, including the per-row error margins -- which vary,
/// because below 1.000 the conversion is not meaningful in either direction and the C++ widens
/// the bars rather than dropping the cases.
#[test]
fn specific_gravity_and_brix_agree_with_the_published_tables() {
    #[derive(serde::Deserialize)]
    struct Row {
        sg: f64,
        brix: f64,
        error_margin_sg_to_brix: f64,
        error_margin_brix_to_sg: f64,
    }
    #[derive(serde::Deserialize)]
    struct Document {
        rows: Vec<Row>,
    }

    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testData/expected/sg-brix-equivalence.json");
    let document: Document =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read expected values")).expect("parse");
    assert_eq!(document.rows.len(), 142);

    for row in &document.rows {
        let to_brix = bt_measure::formula::sg_at_20c_to_brix(row.sg);
        assert!(
            fuzzy(to_brix, row.brix, row.error_margin_sg_to_brix),
            "SG {} -> Brix: got {to_brix}, expected {} +/- {}",
            row.sg,
            row.brix,
            row.error_margin_sg_to_brix
        );
        let to_sg = bt_measure::formula::brix_to_sg_at_20c(row.brix);
        assert!(
            fuzzy(to_sg, row.sg, row.error_margin_brix_to_sg),
            "Brix {} -> SG: got {to_sg}, expected {} +/- {}",
            row.brix,
            row.sg,
            row.error_margin_brix_to_sg
        );
    }
}
