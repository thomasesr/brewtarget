//! Validating `.beer` files against the dotBeer schema.
//!
//! The fixtures in `testData/dotBeer/examples/` are the oracle: everything under `recipes/`,
//! `stock/` and `warnings/` must validate, and everything under `invalid/` must be rejected --
//! though not all of it by the schema, which is the point of the split below.

use bt_format::jsonc;
use bt_format::schema::Schema;

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn dotbeer_schema() -> Schema {
    Schema::load(&repository().join("schemas/dotBeer"), "DotBeer.beer.schema").expect("load the dotBeer schema")
}

fn examples(subdirectory: &str) -> Vec<std::path::PathBuf> {
    let directory = repository().join("testData/dotBeer/examples").join(subdirectory);
    let mut found: Vec<_> = std::fs::read_dir(&directory)
        .unwrap_or_else(|e| panic!("{}: {e}", directory.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.extension().is_some_and(|extension| extension == "beer").then_some(path)
        })
        .collect();
    found.sort();
    found
}

/// The schema is spread across fifteen files that refer to one another by relative name, so
/// loading it at all proves the references resolve.
#[test]
fn the_schema_loads_with_all_its_cross_references() {
    let schema = dotbeer_schema();
    // An empty object is missing everything required, so it must fail -- proving the validator
    // is actually doing something.
    assert!(!schema.is_valid(&serde_json::json!({})));
}

#[test]
fn every_valid_example_validates() {
    let schema = dotbeer_schema();
    let mut checked = 0;
    for subdirectory in ["recipes", "stock", "warnings"] {
        for path in examples(subdirectory) {
            let source = std::fs::read_to_string(&path).expect("read");
            let document = jsonc::value_from_str(&source).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            schema.validate(&document).unwrap_or_else(|e| panic!("{}:\n{e}", path.display()));
            checked += 1;
        }
    }
    assert!(checked >= 5, "only checked {checked} files");
}

/// Half of the invalid fixtures break a rule the schema can express -- a value out of range, a
/// required field missing, two mutually exclusive fields both present.
const REJECTED_BY_THE_SCHEMA: [&str; 8] = [
    "BrewLogFieldErrors.beer",
    "FermentableNoYield.beer",
    "FoundationVolumeErrors.beer",
    "RangesAndLimits.beer",
    "ScheduleTimeAndGravity.beer",
    "StockConsistencyErrors.beer",
    "StockFieldErrors.beer",
    "WaterAgentRules.beer",
];

/// The other half break rules JSON Schema cannot express: a reference to something that is not
/// there, two records claiming the same identifier, dates in the wrong order.  Catching those is
/// a separate pass, which the C++ does in its record classes rather than in the schema.
const NEEDS_SEMANTIC_CHECKS: [&str; 6] = [
    "AdditionStepOutOfRange.beer",
    "BrewLogDatesReversed.beer",
    "DanglingReference.beer",
    "DefaultBatchSizeErrors.beer",
    "DuplicateLocalId.beer",
    "WrongReferenceType.beer",
];

/// And two more that are rejected earlier still, as invalid JSON.
const REJECTED_AS_JSON: [&str; 2] = ["NonFiniteNumber.beer", "TrailingComma.beer"];

/// Two reference fixtures the schema cannot judge either; listed separately because what is
/// wrong with them is which record a reference points at, not its shape.
const NEEDS_REFERENCE_CHECKS: [&str; 2] = ["DuplicateKey.beer", "StockReferenceErrors.beer"];

#[test]
fn every_invalid_fixture_is_accounted_for() {
    let names: Vec<String> =
        examples("invalid").iter().map(|path| path.file_name().expect("name").to_string_lossy().into_owned()).collect();

    let mut classified: Vec<&str> = REJECTED_BY_THE_SCHEMA
        .iter()
        .chain(&NEEDS_SEMANTIC_CHECKS)
        .chain(&REJECTED_AS_JSON)
        .chain(&NEEDS_REFERENCE_CHECKS)
        .copied()
        .collect();
    classified.sort_unstable();

    let mut found: Vec<&str> = names.iter().map(String::as_str).collect();
    found.sort_unstable();
    assert_eq!(found, classified, "every invalid fixture should be classified");
}

#[test]
fn the_schema_rejects_what_it_can_express() {
    let schema = dotbeer_schema();
    for path in examples("invalid") {
        let name = path.file_name().expect("name").to_string_lossy().into_owned();
        if REJECTED_AS_JSON.contains(&name.as_str()) {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read");
        let document = jsonc::value_from_str(&source).unwrap_or_else(|e| panic!("{name}: {e}"));

        if REJECTED_BY_THE_SCHEMA.contains(&name.as_str()) {
            assert!(schema.validate(&document).is_err(), "{name} should fail schema validation");
        } else {
            // Valid against the schema, and wrong for a reason only a later pass can see.
            assert!(schema.is_valid(&document), "{name} is expected to pass the schema");
        }
    }
}

/// A failure report should name every problem, not just the first: a hand-written file usually
/// has several, and fixing them one run at a time is miserable.
#[test]
fn validation_reports_every_violation() {
    let schema = dotbeer_schema();
    let source = std::fs::read_to_string(repository().join("testData/dotBeer/examples/invalid/RangesAndLimits.beer"))
        .expect("read");
    let document = jsonc::value_from_str(&source).expect("parse");
    let error = schema.validate(&document).expect_err("should be rejected");
    let report = error.to_string();
    assert!(report.lines().count() > 2, "expected several violations, got: {report}");
}
