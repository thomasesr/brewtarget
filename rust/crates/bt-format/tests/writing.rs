// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/writing.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Writing whole documents, and reading them back.
//!
//! The property is that a document survives being written and read again unchanged.  The files
//! will not match byte for byte -- they had comments, their own choice of units, their own
//! ordering -- but nothing the database stores may be lost or altered on the way through.

use bt_format::document::{read_beerjson, read_beerxml, read_dotbeer, write_beerjson, write_beerxml, write_dotbeer};
use bt_format::{jsonc, xml};

fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read_text(relative: &str) -> String {
    let path = repository().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// BeerXML declares ISO-8859-1, where every byte is one character.
fn read_latin1(relative: &str) -> String {
    let path = repository().join(relative);
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .iter()
        .map(|byte| *byte as char)
        .collect()
}

#[test]
fn a_dotbeer_document_survives_being_written() {
    let parsed =
        jsonc::value_from_str(&read_text("testData/dotBeer/examples/recipes/AmericanPaleAle.beer")).expect("parse");
    let document = read_dotbeer(&parsed).expect("read");

    let written = write_dotbeer(&document).expect("write");
    let again = read_dotbeer(&written).expect("read back");

    assert_eq!(again.len(), document.len());
    assert_eq!(again.recipes.len(), 1);
    assert_eq!(again, document, "a .beer document changed on the way through");
}

/// A recipe's references are local IDs, not columns, so they have to be written back explicitly
/// -- the first version dropped all seven of them and the recipe came back pointing at nothing.
#[test]
fn a_recipes_references_survive_being_written() {
    let parsed =
        jsonc::value_from_str(&read_text("testData/dotBeer/examples/recipes/AmericanPaleAle.beer")).expect("parse");
    let document = read_dotbeer(&parsed).expect("read");
    let again = read_dotbeer(&write_dotbeer(&document).expect("write")).expect("read back");

    let references = &again.recipes[0].record.references;
    assert_eq!(references.len(), 7, "{references:?}");
    for field in ["style_id", "equipment_id", "mash_id", "boil_id", "fermentation_id"] {
        let reference = references.get(field).unwrap_or_else(|| panic!("no {field}"));
        assert!(again.by_local_id(reference).is_some(), "{field} points at nothing");
    }
}

#[test]
fn the_shipped_beerjson_library_survives_being_written() {
    let parsed =
        jsonc::value_from_str(&read_text("data/DefaultContent003-Ingredients-Hops-Yeasts.json")).expect("parse");
    let document = read_beerjson(&parsed).expect("read");
    assert_eq!(document.len(), 578);

    let again = read_beerjson(&write_beerjson(&document).expect("write")).expect("read back");
    assert_eq!(again, document, "578 records changed on the way through");
}

/// BeerXML is the hardest of the three: it has no single root, writes booleans in capitals, uses
/// its own enum spellings, and embeds a recipe's ingredients inside the additions that use them.
#[test]
fn the_shipped_beerxml_library_survives_being_written() {
    let parsed = xml::read(&read_latin1("data/DefaultContent001-OriginalDefaultData.xml")).expect("parse");
    let document = read_beerxml(&parsed).expect("read");
    assert_eq!(document.len(), 547);
    assert_eq!(document.recipes.len(), 29);

    let text = xml::write(&write_beerxml(&document).expect("write"));
    let again = read_beerxml(&xml::read(&text).expect("parse what we wrote")).expect("read back");
    assert_eq!(again, document, "547 records changed on the way through");
}

/// What is written is a BeerXML file, not our internal shape: the synthetic root stays hidden,
/// and booleans are capitalised the way the standard says.
#[test]
fn what_is_written_looks_like_beerxml() {
    let parsed = xml::read(&read_latin1("data/DefaultContent001-OriginalDefaultData.xml")).expect("parse");
    let document = read_beerxml(&parsed).expect("read");
    let text = xml::write(&write_beerxml(&document).expect("write"));

    assert!(text.starts_with("<?xml version="), "a BeerXML file begins with a declaration");
    assert!(!text.contains("<BEER_XML>"), "the synthetic root is ours, not the document's");
    assert!(text.contains("<FERMENTABLES>") && text.contains("<FERMENTABLE>"));
    assert!(text.contains("<RECIPES>") && text.contains("<RECIPE>"));

    // Enum values go out in BeerXML's own spelling, not the database's.
    assert!(text.contains("<TYPE>Grain</TYPE>"), "fermentable types should be capitalised");
    assert!(!text.contains("<TYPE>grain</TYPE>"), "the database spelling should not escape");
    // And booleans in capitals, which the standard specifies and XML does not allow.
    assert!(text.contains("TRUE") || text.contains("FALSE"));
}

#[test]
fn xml_escaping_is_applied() {
    let document = serde_json::json!({"BEER_XML": {"NOTES": "hops & \"malt\" <here>"}});
    let text = xml::write(&document);
    assert!(text.contains("hops &amp; &quot;malt&quot; &lt;here&gt;"), "{text}");
    // And it comes back as it went in.
    let parsed = xml::read(&text).expect("parse");
    assert_eq!(parsed["BEER_XML"]["NOTES"], "hops & \"malt\" <here>");
}

/// A library read in one format and written in another keeps everything.
///
/// This is the bug these tests did not have: every round trip was within one format, where the
/// collection names happen to match whatever read them.  Across formats they do not -- a hop is
/// in `hops` in a `.beer` file, `hop_varieties` in BeerJSON and `<HOPS>` in BeerXML -- and the
/// writer silently wrote nothing for every collection it could not find under its own name.
/// Exporting a library as BeerXML produced recipes and no ingredients at all.
#[test]
fn a_library_survives_being_written_in_a_different_format() {
    let parsed =
        jsonc::value_from_str(&read_text("data/DefaultContent003-Ingredients-Hops-Yeasts.json")).expect("parse");
    let document = read_beerjson(&parsed).expect("read");

    let counts = |document: &bt_format::document::Document| {
        let mut counts: Vec<(String, usize)> =
            document.collections.iter().map(|(table, records)| (table.clone(), records.len())).collect();
        counts.sort();
        counts
    };
    let before = counts(&document);
    assert!(before.iter().any(|(table, count)| table == "hop" && *count > 0), "{before:?}");

    // BeerJSON in, `.beer` out, and back again.
    let as_dotbeer = read_dotbeer(&write_dotbeer(&document).expect("write .beer")).expect("read it back");
    assert_eq!(counts(&as_dotbeer), before, "a .beer file lost records BeerJSON had");

    // And BeerXML, which names its collections differently again.
    let written = xml::write(&write_beerxml(&document).expect("write BeerXML"));
    let as_beerxml = read_beerxml(&xml::read(&written).expect("parse")).expect("read it back");
    for (table, count) in &before {
        // BeerXML has no section for some of what the other two carry; what it does carry, it
        // has to carry all of.
        if let Some(found) = as_beerxml.records_of(table) {
            assert_eq!(found.len(), *count, "BeerXML lost {table} records");
        }
    }
    assert!(as_beerxml.records_of("hop").is_some_and(|hops| !hops.is_empty()), "BeerXML wrote no hops at all");
}
