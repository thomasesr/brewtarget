// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/references.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Checks on local IDs and the references between them.
//!
//! The fixtures under `testData/dotBeer/examples/invalid/` come from the DotBeer project itself
//! and each carries an `EXPECT-INVALID:` line saying what should be rejected and where.  Those
//! lines are the oracle here: this is a third-party conformance suite rather than a restatement
//! of what this code does.

use bt_format::jsonc;
use bt_format::references::{ReferenceRules, check, definitions};

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn rules() -> ReferenceRules {
    ReferenceRules::from_schema_directory(&repository().join("schemas/dotBeer")).expect("read the schema directory")
}

fn example(subdirectory: &str, name: &str) -> serde_json::Value {
    let path = repository().join("testData/dotBeer/examples").join(subdirectory).join(name);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    jsonc::value_from_str(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The rules come from the schema's own `x-refers-to` annotations rather than a hand-written
/// list, so a new reference field is picked up without anyone remembering to add it here.
#[test]
fn the_reference_rules_come_from_the_schema() {
    let rules = rules();
    assert!(rules.len() >= 10, "only found {} reference rules", rules.len());
    assert_eq!(rules.target_of("style_id"), Some("/DotBeer/styles"));
    assert_eq!(rules.target_of("hop_id"), Some("/DotBeer/hops"));
    assert_eq!(rules.target_of("brew_log_id"), Some("/DotBeer/recipes/*/brew_logs"));
    assert_eq!(rules.target_of("name"), None, "an ordinary field is not a reference");
}

#[test]
fn the_valid_examples_have_no_reference_problems() {
    let rules = rules();
    for (subdirectory, name) in [
        ("recipes", "AmericanPaleAle.beer"),
        ("recipes", "DrySaisonSubUnityFG.beer"),
        ("recipes", "GermanPilsColdCrash.beer"),
        ("stock", "StockPurchases.beer"),
        ("warnings", "DefaultBatchSizeTooBig.beer"),
    ] {
        let problems = check(&example(subdirectory, name), &rules);
        assert!(problems.is_empty(), "{name}: {problems:?}");
    }
}

/// Rule (a) from the schema's own description: local IDs are unique across the whole file, even
/// between objects of different types.
#[test]
fn a_local_id_may_not_be_claimed_twice() {
    let document = example("invalid", "DuplicateLocalId.beer");
    let (_, duplicates) = definitions(&document);
    assert_eq!(duplicates.len(), 1, "{duplicates:?}");
    assert!(duplicates[0].message.contains("\"pale\""), "{}", duplicates[0]);
    // The fixture puts the clash between a hop and a fermentable deliberately.
    assert!(duplicates[0].path.starts_with("/DotBeer/hops/"), "{}", duplicates[0]);
    assert!(duplicates[0].message.contains("/DotBeer/fermentables/"), "{}", duplicates[0]);
}

/// Rule (b): every reference resolves to something defined in the same file.
#[test]
fn a_reference_must_point_at_something() {
    let problems = check(&example("invalid", "DanglingReference.beer"), &rules());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "/DotBeer/recipes/0/ingredients/hop_additions/0/hop_id");
    assert!(problems[0].message.contains("\"hop_citra\" is not defined"), "{}", problems[0]);
}

/// Rule (c): a reference names the right kind of thing.  A recipe's style has to be a style.
#[test]
fn a_reference_must_point_at_the_right_kind_of_thing() {
    let problems = check(&example("invalid", "WrongReferenceType.beer"), &rules());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].path, "/DotBeer/recipes/0/style_id");
    assert!(problems[0].message.contains("must refer to an item in /DotBeer/styles"), "{}", problems[0]);
    assert!(problems[0].message.contains("is in /DotBeer/hops"), "{}", problems[0]);
}

/// A target may name a nested collection -- brew logs live inside whichever recipe they belong
/// to -- which the wildcard in `/DotBeer/recipes/*/brew_logs` is for.
#[test]
fn references_into_nested_collections_are_checked_too() {
    let problems = check(&example("invalid", "StockReferenceErrors.beer"), &rules());
    assert!(problems.len() >= 3, "{problems:?}");
    assert!(problems.iter().any(|problem| problem.message.contains("/DotBeer/recipes/*/brew_logs")), "{problems:?}");
}
