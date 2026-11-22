//! Reading BeerJSON.
//!
//! The oracle here is `data/DefaultContent*.json`, which is the ingredient library Brewtarget
//! ships: 282 hops, 571 cultures and 110 styles of real data, in the format other software
//! actually writes.  Nothing in this file is hand-made for the test.

use bt_format::document::read_beerjson;
use bt_format::jsonc;
use bt_model::enums::{HopType, YeastForm};
use bt_model::tables::{HopRow, StyleRow, YeastRow};

fn content(name: &str) -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data").join(name);
    jsonc::value_from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// BeerJSON names several collections differently from `.beer` -- hops are `hop_varieties`,
/// miscs are `miscellaneous_ingredients`, waters are `profiles` -- which is the sort of thing the
/// extracted record definitions exist so nobody has to remember.
#[test]
fn the_shipped_ingredient_library_reads() {
    let document = read_beerjson(&content("DefaultContent003-Ingredients-Hops-Yeasts.json")).expect("read");
    assert_eq!(document.len(), 578);

    let hops = document.collections.iter().find(|(name, _)| name == "hop").expect("hops");
    assert_eq!(hops.1.len(), 282);
    assert!(hops.1.iter().all(|hop| hop.record.table == "hop"));

    let cultures = document.collections.iter().find(|(name, _)| name == "yeast").expect("cultures");
    assert_eq!(cultures.1.len(), 296);
    assert!(cultures.1.iter().all(|culture| culture.record.table == "yeast"));
}

#[test]
fn a_hop_from_the_library_has_its_values() {
    let document = read_beerjson(&content("DefaultContent003-Ingredients-Hops-Yeasts.json")).expect("read");
    let (_, hops) = document.collections.iter().find(|(name, _)| name == "hop").expect("hops");

    let row: HopRow = hops[0].record.clone().into_row().expect("deserialise");
    assert_eq!(row.name.as_deref(), Some("Adeena"));
    assert_eq!(row.alpha_pct, Some(4.3));
    assert_eq!(row.beta_pct, Some(3.5));
    assert_eq!(row.origin.as_deref(), Some("United States of America (USA)"));
    assert_eq!(row.htype().expect("valid type"), Some(HopType::Aroma));
    // The oil fractions come through too, which is a good sign the whole mapping is wired up.
    assert_eq!(row.myrcene_pct, Some(28.5));
    assert_eq!(row.total_oil_ml_per_100g, Some(1.0));
}

#[test]
fn every_culture_and_style_deserialises() {
    let ingredients = read_beerjson(&content("DefaultContent003-Ingredients-Hops-Yeasts.json")).expect("read");
    let (_, cultures) = ingredients.collections.iter().find(|(name, _)| name == "yeast").expect("cultures");
    for culture in cultures {
        let row: YeastRow = culture.record.clone().into_row().expect("deserialise a culture");
        assert!(row.name.is_some(), "every culture has a name");
        // A form the database does not recognise would be a mapping error, not missing data.
        row.form().expect("the form should be one we know");
    }

    let styles = read_beerjson(&content("DefaultContent002-BJCP_2021_Styles.json")).expect("read");
    let (_, styles) = styles.collections.iter().find(|(name, _)| name == "style").expect("styles");
    assert_eq!(styles.len(), 110);
    for style in styles {
        let row: StyleRow = style.record.clone().into_row().expect("deserialise a style");
        assert!(row.name.is_some());
    }
}

#[test]
fn the_second_yeast_file_reads_too() {
    let document = read_beerjson(&content("DefaultContent004-MoreYeasts.json")).expect("read");
    let (_, cultures) = document.collections.iter().find(|(name, _)| name == "yeast").expect("cultures");
    assert_eq!(cultures.len(), 275);
    let forms: Vec<YeastForm> =
        cultures.iter().filter_map(|c| c.record.clone().into_row::<YeastRow>().ok()?.form().ok()?).collect();
    assert!(forms.len() > 200, "most cultures say what form they come in");
}
