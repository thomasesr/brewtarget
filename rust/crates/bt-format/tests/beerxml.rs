// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/tests/beerxml.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Reading BeerXML.
//!
//! The oracle is `data/DefaultContent001-OriginalDefaultData.xml`, the library Brewtarget has
//! shipped since before BeerJSON existed: 547 records across eight sections, including 29
//! recipes.

use bt_format::document::read_beerxml;
use bt_format::enums as format_enums;
use bt_format::xml;
use bt_model::enums::{HopForm, HopType};
use bt_model::tables::HopRow;

fn default_content() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../data/DefaultContent001-OriginalDefaultData.xml");
    let raw = std::fs::read(&path).expect("read the default content");
    // The file declares ISO-8859-1, where every byte is one character.
    let text: String = raw.iter().map(|byte| *byte as char).collect();
    xml::read(&text).expect("parse")
}

/// BeerXML 1.0 is not valid XML: it omits the single root element every XML document needs.  The
/// C++ inserts one after the declaration, and so does this.
#[test]
fn a_beerxml_document_is_given_the_root_it_lacks() {
    let wrapped = xml::wrap("<?xml version=\"1.0\"?>\n<HOPS><HOP><NAME>Saaz</NAME></HOP></HOPS>").expect("wrap");
    assert!(wrapped.contains("<BEER_XML>"), "{wrapped}");
    assert!(wrapped.ends_with("</BEER_XML>"), "{wrapped}");
    // The declaration stays on line one, so reported line numbers are one more than the file's.
    assert!(wrapped.lines().next().expect("a first line").starts_with("<?xml"));

    // Something that is not XML at all is refused rather than wrapped.
    assert!(xml::wrap("hello").is_err());
}

#[test]
fn the_shipped_library_reads() {
    let document = read_beerxml(&default_content()).expect("read");
    assert_eq!(document.len(), 547);
    assert_eq!(document.recipes.len(), 29);

    for (section, table, expected) in [
        ("FERMENTABLES", "fermentable", 248),
        ("HOPS", "hop", 62),
        ("MISCS", "misc", 84),
        ("YEASTS", "yeast", 8),
        ("WATERS", "water", 14),
        ("STYLES", "style", 98),
        ("EQUIPMENTS", "equipment", 4),
    ] {
        // Keyed by table: the document is format-neutral, so `<FERMENTABLES>` and `.beer`'s
        // `fermentables` both arrive as `fermentable`.
        let (_, records) = document.collections.iter().find(|(name, _)| name == table).expect(section);
        assert_eq!(records.len(), expected, "{section}");
        assert!(records.iter().all(|record| record.record.table == table));
    }
}

/// BeerXML supports fewer hop forms and malt types than Brewtarget, so it declares its own
/// mappings with its own capitalisation.  Writing its spelling into the database would produce a
/// row the application could not read back, so values are translated through the enum member the
/// two mappings agree on.
#[test]
fn beerxml_enum_values_are_translated_to_the_ones_the_database_stores() {
    let document = read_beerxml(&default_content()).expect("read");
    let (_, hops) = document.collections.iter().find(|(name, _)| name == "hop").expect("hops");

    let row: HopRow = hops[0].record.clone().into_row().expect("deserialise");
    assert_eq!(row.name.as_deref(), Some("Agnus"));
    // The file says "Pellet" and "Bittering"; the database says "pellet" and "bittering".
    assert_eq!(row.form.as_deref(), Some("pellet"));
    assert_eq!(row.htype.as_deref(), Some("bittering"));
    assert_eq!(row.form().expect("a form we know"), Some(HopForm::Pellet));
    assert_eq!(row.htype().expect("a type we know"), Some(HopType::Bittering));
}

#[test]
fn the_translation_goes_both_ways() {
    assert_eq!(format_enums::to_database_owned("BEER_XML_HOP_FORM_MAPPER", "Pellet").as_deref(), Some("pellet"));
    assert_eq!(format_enums::from_database("BEER_XML_HOP_FORM_MAPPER", "pellet").as_deref(), Some("Pellet"));

    // BeerXML has no word for a wet leaf, so Brewtarget writes one inside an XML comment: other
    // software reads plain "Leaf" and we read back what we meant.
    assert_eq!(
        format_enums::from_database("BEER_XML_HOP_FORM_MAPPER", "leaf (wet)").as_deref(),
        Some("Leaf<!--WetLeaf-->")
    );
    assert_eq!(
        format_enums::to_database_owned("BEER_XML_HOP_FORM_MAPPER", "Leaf<!--WetLeaf-->").as_deref(),
        Some("leaf (wet)")
    );

    // A format that uses the model's own mapping needs no translation at all.
    assert_eq!(format_enums::to_database_owned("Hop::formStringMapping", "pellet").as_deref(), Some("pellet"));
    // And a value the mapping does not know is refused rather than guessed at.
    assert_eq!(format_enums::to_database_owned("BEER_XML_HOP_FORM_MAPPER", "Gravel"), None);
}

#[test]
fn every_hop_in_the_library_deserialises() {
    let document = read_beerxml(&default_content()).expect("read");
    let (_, hops) = document.collections.iter().find(|(name, _)| name == "hop").expect("hops");
    for hop in hops {
        let row: HopRow = hop.record.clone().into_row().expect("deserialise");
        assert!(row.name.is_some());
        row.form().expect("the form should be one we know");
        row.htype().expect("the type should be one we know");
    }
}
