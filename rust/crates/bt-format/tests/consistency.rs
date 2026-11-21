// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/consistency.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Cross-field checks against the DotBeer conformance fixtures.
//!
//! Each fixture states what it expects in an `EXPECT-INVALID:` or `EXPECT-WARNING:` line, which
//! is what these tests are written against.

use bt_format::consistency::{Severity, check, check_addition_steps, check_batch_sizes, has_errors};
use bt_format::jsonc;

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn example(subdirectory: &str, name: &str) -> serde_json::Value {
    let path = repository().join("testData/dotBeer/examples").join(subdirectory).join(name);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    jsonc::value_from_str(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The two batch size fixtures contain identical equipment and differ in one thing only: the
/// version they declare.  dotBeer 1.0.0 rejected a batch that would not fit; 1.1.0 downgraded it
/// to a warning, since a manufacturer's nominal batch size often assumes a shorter boil and the
/// file is perfectly readable either way.
#[test]
fn the_same_oversized_batch_is_an_error_or_a_warning_depending_on_the_version() {
    let errors = check_batch_sizes(&example("invalid", "DefaultBatchSizeErrors.beer"));
    let warnings = check_batch_sizes(&example("warnings", "DefaultBatchSizeTooBig.beer"));

    assert_eq!(errors.len(), 3, "{errors:?}");
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(errors.iter().all(|finding| finding.severity == Severity::Error));
    assert!(warnings.iter().all(|finding| finding.severity == Severity::Warning));

    // Same three equipments, same arithmetic, same wording.
    let message_of = |findings: &[bt_format::consistency::Finding]| -> Vec<String> {
        findings.iter().map(|finding| finding.message.clone()).collect()
    };
    assert_eq!(message_of(&errors), message_of(&warnings));
}

#[test]
fn the_batch_size_arithmetic_matches_the_expected_figures() {
    let findings = check_batch_sizes(&example("invalid", "DefaultBatchSizeErrors.beer"));

    // 58 + 3 loss + 4.5 boil-off = 65.5, against a 60 litre kettle.
    assert!(findings[0].message.contains("65.5 l"), "{}", findings[0]);
    assert!(findings[0].message.contains("60 minutes' boil-off"), "{}", findings[0]);

    // 5 US gallons is 18.9271 litres, so + 1 + 4 = 23.9271 against a 22 litre kettle.  Getting
    // the unit conversion wrong would show up here and nowhere else.
    assert!(findings[1].message.contains("23.9271 l"), "{}", findings[1]);

    // With no boil-off rate there is nothing to add for it, and the message says so.
    assert!(findings[2].message.contains("32 l"), "{}", findings[2]);
    assert!(!findings[2].message.contains("boil-off"), "{}", findings[2]);
}

/// The fourth equipment in that fixture fills its kettle exactly, which is fine: 25 + 2 + 3 = 30
/// against a 30 litre kettle.  The comparison has to be strict.
#[test]
fn a_kettle_filled_exactly_is_not_a_problem() {
    let findings = check_batch_sizes(&example("invalid", "DefaultBatchSizeErrors.beer"));
    assert!(
        !findings.iter().any(|finding| finding.path.ends_with("/3")),
        "the 'Exactly Full' equipment should pass: {findings:?}"
    );
}

#[test]
fn an_addition_cannot_name_a_step_that_does_not_exist() {
    let findings = check_addition_steps(&example("invalid", "AdditionStepOutOfRange.beer"));
    assert_eq!(findings.len(), 2, "{findings:?}");

    // Step 3 of a mash that has one step.
    assert!(
        findings.iter().any(|finding| finding.message.contains("step 3 of add_to_mash")
            && finding.message.contains("\"Single Step\" only has 1 step(s)")),
        "{findings:?}"
    );
    // And a fermentation stage on a recipe that has no fermentation at all.
    assert!(
        findings.iter().any(|finding| finding.message.contains("step 2 of add_to_fermentation")
            && finding.message.contains("has no fermentation_id")),
        "{findings:?}"
    );
    assert!(has_errors(&findings));
}

#[test]
fn the_valid_examples_pass_every_cross_field_check() {
    for (subdirectory, name) in [
        ("recipes", "AmericanPaleAle.beer"),
        ("recipes", "DrySaisonSubUnityFG.beer"),
        ("recipes", "GermanPilsColdCrash.beer"),
        ("stock", "StockPurchases.beer"),
    ] {
        let findings = check(&example(subdirectory, name));
        assert!(findings.is_empty(), "{name}: {findings:?}");
    }
}

/// The warnings fixture is meant to load: it has something worth saying, but nothing fatal.
#[test]
fn a_warning_is_not_a_reason_to_reject_a_file() {
    let findings = check(&example("warnings", "DefaultBatchSizeTooBig.beer"));
    assert!(!findings.is_empty(), "there is something to say about it");
    assert!(!has_errors(&findings), "but nothing that should stop it loading");
}
