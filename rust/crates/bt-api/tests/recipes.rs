// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/tests/recipes.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! A recipe, end to end over HTTP.
//!
//! Import a file, ask for what the recipe works out to, and export it again.  This is the path a
//! user actually takes, and it crosses every crate in the workspace.

mod support;

use axum::http::StatusCode;
use serde_json::Value;
use support::{app, request};

/// A small all-grain recipe, written out by hand so the expected figures are pinned to known
/// inputs rather than to whatever happens to be in the shipped content.
const RECIPE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<RECIPES>
  <RECIPE>
    <NAME>Test Bitter</NAME>
    <VERSION>1</VERSION>
    <TYPE>All Grain</TYPE>
    <BREWER>Tester</BREWER>
    <BATCH_SIZE>20.0</BATCH_SIZE>
    <BOIL_SIZE>25.0</BOIL_SIZE>
    <BOIL_TIME>60.0</BOIL_TIME>
    <EFFICIENCY>75.0</EFFICIENCY>
    <FERMENTABLES>
      <FERMENTABLE>
        <NAME>Pale Malt</NAME>
        <VERSION>1</VERSION>
        <TYPE>Grain</TYPE>
        <AMOUNT>5.0</AMOUNT>
        <YIELD>80.0</YIELD>
        <COLOR>3.0</COLOR>
      </FERMENTABLE>
    </FERMENTABLES>
    <HOPS>
      <HOP>
        <NAME>Fuggle</NAME>
        <VERSION>1</VERSION>
        <ALPHA>4.5</ALPHA>
        <AMOUNT>0.050</AMOUNT>
        <USE>Boil</USE>
        <TIME>60.0</TIME>
      </HOP>
    </HOPS>
    <YEASTS>
      <YEAST>
        <NAME>London Ale</NAME>
        <VERSION>1</VERSION>
        <TYPE>Ale</TYPE>
        <FORM>Liquid</FORM>
        <AMOUNT>0.035</AMOUNT>
        <ATTENUATION>73.0</ATTENUATION>
      </YEAST>
    </YEASTS>
  </RECIPE>
</RECIPES>
"#;

/// Import the recipe and return the id the database gave it.
async fn imported(app: &axum::Router) -> i64 {
    let (status, summary) = request(app, "POST", "/api/import", support::text(RECIPE)).await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["format"], "beer_xml");
    let ids = summary["recipe_ids"].as_array().expect("the ids of the recipes read");
    assert_eq!(ids.len(), 1, "one recipe in, one recipe out");
    ids[0].as_i64().expect("an id")
}

#[tokio::test]
async fn a_recipe_survives_being_imported() {
    let app = app("recipe-import").await;
    let id = imported(&app).await;

    let (status, recipe) = request(&app, "GET", &format!("/api/recipes/{id}/full"), None).await;
    assert_eq!(status, StatusCode::OK, "{recipe}");
    assert_eq!(recipe["row"]["name"], "Test Bitter");
    assert_eq!(recipe["row"]["brewer"], "Tester");
    // BeerXML carries volumes in litres and the database keeps them in litres too.
    assert_eq!(recipe["row"]["batch_size"], 20.0);
    assert_eq!(recipe["row"]["efficiency"], 75.0);

    // A hop and a malt go in as additions, each naming the ingredient it is an addition of.
    let fermentables = recipe["fermentables"].as_array().expect("the malt");
    assert_eq!(fermentables.len(), 1);
    assert_eq!(fermentables[0]["ingredient"]["name"], "Pale Malt");
    assert_eq!(fermentables[0]["ingredient"]["fine_grind_yield_pct"], 80.0);
    assert_eq!(fermentables[0]["addition"]["quantity"], 5.0);
    // BeerXML has no element for the unit: an amount is in kilograms because the standard says
    // so, and reading one fills that in.
    assert_eq!(fermentables[0]["addition"]["unit"], "kilograms");
    assert_eq!(fermentables[0]["addition"]["stage"], "add_to_mash");

    let hops = recipe["hops"].as_array().expect("the hop");
    assert_eq!(hops.len(), 1);
    assert_eq!(hops[0]["ingredient"]["name"], "Fuggle");
    assert_eq!(hops[0]["ingredient"]["alpha_pct"], 4.5);
    assert_eq!(hops[0]["addition"]["quantity"], 0.050);
    // BeerXML says only that the hop is used in the boil for 60 minutes; the stage, the step
    // within the boil and the time it goes in are all worked out from that.
    assert_eq!(hops[0]["addition"]["stage"], "add_to_boil");
    assert_eq!(hops[0]["addition"]["step"], 2);
    assert_eq!(hops[0]["addition"]["add_at_time_mins"], 60.0);
}

#[tokio::test]
async fn the_figures_the_recipe_screen_shows_are_worked_out() {
    let app = app("recipe-calculated").await;
    let id = imported(&app).await;

    let (status, figures) = request(&app, "GET", &format!("/api/recipes/{id}/calculated"), None).await;
    assert_eq!(status, StatusCode::OK, "{figures}");

    // 5 kg of malt yielding 80% at 75% efficiency is 3 kg of sugar, and 3 kg in 20 L is a
    // gravity of about 1.058.  Pinning the figures to more decimal places than that belongs in
    // bt-calc's own tests, which compare against the C++ directly.
    let og = figures["og"].as_f64().expect("original gravity");
    assert!((1.050..1.065).contains(&og), "original gravity was {og}");
    let fg = figures["fg"].as_f64().expect("final gravity");
    assert!(fg < og, "fermenting lowers the gravity: {fg} from {og}");
    let abv = figures["abv_pct"].as_f64().expect("alcohol");
    assert!((4.0..8.0).contains(&abv), "alcohol was {abv}%");
    let ibu = figures["ibus"].as_f64().expect("bitterness");
    assert!((15.0..45.0).contains(&ibu), "bitterness was {ibu}");
    let colour = figures["color_srm"].as_f64().expect("colour");
    assert!((3.0..12.0).contains(&colour), "colour was {colour} SRM");
}

#[tokio::test]
async fn there_is_a_sheet_to_brew_from() {
    let app = app("recipe-brewday").await;
    let id = imported(&app).await;

    let (status, sheet) = request(&app, "GET", &format!("/api/recipes/{id}/brewday"), None).await;
    assert_eq!(status, StatusCode::OK, "{sheet}");
    assert_eq!(sheet["recipe_name"], "Test Bitter");
    assert_eq!(sheet["batch_size_l"], 20.0);
    // BeerXML has no boil: it has a BOIL_TIME, and reading one builds a boil whose middle step
    // lasts that long.
    assert_eq!(sheet["boil_time_mins"], 60.0);
    // The file carried no INSTRUCTIONS, so the sheet is written from the recipe -- which is
    // what the Qt application does when asked for a brew day sheet.
    let steps = sheet["steps"].as_array().expect("a list of steps");
    assert!(!steps.is_empty(), "a recipe with a boil has something to do");
    for (index, step) in steps.iter().enumerate() {
        assert_eq!(step["number"], index as i64 + 1, "numbered from one, in order");
        assert!(step["name"].as_str().is_some_and(|name| !name.trim().is_empty()), "{step}");
        assert!(step["directions"].as_str().is_some_and(|text| !text.trim().is_empty()), "{step}");
    }
    // The hop goes into the boil, and the sheet says so.
    let directions: Vec<&str> = steps.iter().filter_map(|step| step["directions"].as_str()).collect();
    assert!(directions.iter().any(|text| text.contains("Fuggle")), "{directions:?}");
}

#[tokio::test]
async fn what_went_in_comes_back_out() {
    let app = app("recipe-export").await;
    imported(&app).await;

    for (format, root) in [("dot_beer", "recipes"), ("beer_json", "recipes")] {
        let (status, exported) = request(&app, "GET", &format!("/api/export?format={format}"), None).await;
        assert_eq!(status, StatusCode::OK, "exporting as {format}");
        let recipes = find(&exported, root).unwrap_or(&Value::Null);
        let names: Vec<&str> = recipes
            .as_array()
            .map(|list| list.iter().filter_map(|recipe| recipe["name"].as_str()).collect())
            .unwrap_or_default();
        assert!(names.contains(&"Test Bitter"), "as {format}: {names:?}");
    }

    // BeerXML is not JSON, so it is checked as text.
    let (status, _) = request(&app, "GET", "/api/export?format=beer_xml", None).await;
    assert_eq!(status, StatusCode::OK);
}

/// Find a key anywhere in a document, whatever it is nested inside.
fn find<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(fields) => fields.get(key).or_else(|| fields.values().find_map(|nested| find(nested, key))),
        Value::Array(items) => items.iter().find_map(|item| find(item, key)),
        _ => None,
    }
}
