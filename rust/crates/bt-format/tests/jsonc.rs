// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/jsonc.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Checks on comment-tolerant JSON parsing.
//!
//! The point of blanking comments rather than removing them is that a parse error still points at
//! the line and column of the file the user wrote, so these tests check positions as well as
//! values.

use bt_format::jsonc;

#[test]
fn line_comments_are_ignored() {
    let value: serde_json::Value = jsonc::from_str(
        r#"{
          // the hop we are using
          "name": "Saaz", // with a trailing comment
          "alpha": 3.5
        }"#,
    )
    .expect("parse");
    assert_eq!(value["name"], "Saaz");
    assert_eq!(value["alpha"], 3.5);
}

#[test]
fn block_comments_are_ignored() {
    let value: serde_json::Value =
        jsonc::from_str("{/* opening */ \"a\": 1, /* middle\n   spanning lines */ \"b\": 2}").expect("parse");
    assert_eq!(value["a"], 1);
    assert_eq!(value["b"], 2);
}

/// A comment marker inside a string is part of the string.
#[test]
fn comment_markers_inside_strings_are_left_alone() {
    let value: serde_json::Value =
        jsonc::from_str(r#"{"name": "Simcoe // 2024", "notes": "see /* this */"}"#).expect("parse");
    assert_eq!(value["name"], "Simcoe // 2024");
    assert_eq!(value["notes"], "see /* this */");

    // Including when the quote before it is escaped.
    let value: serde_json::Value = jsonc::from_str(r#"{"a": "say \"hi\" // not a comment"}"#).expect("parse");
    assert_eq!(value["a"], r#"say "hi" // not a comment"#);
}

/// Blanking rather than removing keeps every later byte where it was, so an error in a commented
/// file still points at the right place.
#[test]
fn positions_survive_comment_removal() {
    let source = "{\n  // a comment that is quite long\n  \"a\": 1,\n  \"b\": nope\n}";
    let error = jsonc::value_from_str(source).expect_err("nope is not valid JSON");
    assert_eq!(error.line(), 4, "{error}");

    // And the blanked text is the same length as what it replaced.
    assert_eq!(jsonc::blank_comments(source).len(), source.len());
}

#[test]
fn multi_byte_characters_are_not_split() {
    let value: serde_json::Value =
        jsonc::from_str("{\n  // naming it: German – Noble\n  \"folder\": \"/Hops/German – Noble\"\n}").expect("parse");
    assert_eq!(value["folder"], "/Hops/German – Noble");
}

fn example_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testData/dotBeer/examples");
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).unwrap_or_else(|e| panic!("{}: {e}", directory.display())) {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "beer") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Two of the `invalid/` fixtures are invalid as JSON rather than against the schema, so they are
/// rejected here rather than later: a literal `NaN`, which JSON has no way to write, and a
/// trailing comma.  The C++ rejects both the same way -- it enables Boost's `allow_comments` but
/// not `allow_trailing_commas`.
const JSON_LEVEL_FAILURES: [&str; 2] = ["NonFiniteNumber.beer", "TrailingComma.beer"];

#[test]
fn every_dotbeer_example_parses_except_the_two_that_are_meant_not_to() {
    let files = example_files();
    assert!(files.len() >= 20, "only found {} example files", files.len());

    for path in files {
        let name = path.file_name().expect("file name").to_string_lossy().into_owned();
        let source = std::fs::read_to_string(&path).expect("read");
        let result = jsonc::value_from_str(&source);
        if JSON_LEVEL_FAILURES.contains(&name.as_str()) {
            assert!(result.is_err(), "{name} should not parse as JSON");
        } else {
            result.unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}

/// The rest of the `invalid/` fixtures are perfectly good JSON; what is wrong with them is
/// semantic, and catching that is the schema's job rather than the parser's.
#[test]
fn most_invalid_fixtures_are_valid_json() {
    let invalid: Vec<_> = example_files()
        .into_iter()
        .filter(|path| path.parent().is_some_and(|parent| parent.ends_with("invalid")))
        .collect();
    assert!(invalid.len() >= 15, "only found {} invalid fixtures", invalid.len());

    let parsed = invalid
        .iter()
        .filter(|path| jsonc::value_from_str(&std::fs::read_to_string(path).expect("read")).is_ok())
        .count();
    assert_eq!(parsed, invalid.len() - JSON_LEVEL_FAILURES.len());
}
