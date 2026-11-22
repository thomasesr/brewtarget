// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/tests/library.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The content that ships with the application, served over HTTP.
//!
//! This is the broadest test in the workspace: it seeds a database from the six files in `data/`,
//! then asks the API what those recipes work out to.  Every crate is involved, and the figures
//! are checked against what the styles say they should be -- an American IPA that comes out at
//! 1.020 or 200 IBU is wrong however well each formula tests on its own.

mod support;

use axum::http::StatusCode;
use bt_api::{AppState, api};
use bt_schema::Namespace;
use bt_store::Store;
use serde_json::Value;
use support::{repository, request};

/// A database with the shipped content in it, behind the real router.
///
/// Seeding takes about half a second, and each test gets its own so they can run at the same
/// time without treading on one another.
async fn library(label: &str) -> axum::Router {
    let path = std::env::temp_dir().join(format!("bt-api-library-{label}.sqlite"));
    let _ = std::fs::remove_file(&path);
    std::fs::File::create(&path).expect("create the database file");
    let store = Store::connect(&format!("sqlite://{}", path.display())).await.expect("connect");
    store.prepare(&Namespace::Default).await.expect("prepare");
    let data = repository().join("data");
    bt_store::default_content::load(&store, &data).await.expect("load the default content");
    api().with_state(AppState::new(store, data))
}

/// Find a recipe by name, which is steadier than relying on the order rows come back in.
async fn recipe_named(app: &axum::Router, name: &str) -> i64 {
    let (status, recipes) = request(app, "GET", "/api/recipe", None).await;
    assert_eq!(status, StatusCode::OK);
    recipes
        .as_array()
        .expect("a list of recipes")
        .iter()
        .find(|recipe| recipe["name"] == name)
        .unwrap_or_else(|| panic!("no recipe called {name}"))["id"]
        .as_i64()
        .expect("an id")
}

#[tokio::test]
async fn the_shipped_recipes_come_out_as_their_styles_describe_them() {
    let app = library("styles").await;

    // Gravity, alcohol, colour and bitterness for three recipes that between them cover a pale
    // hoppy beer, a milder one and a strong one.  The ranges are the styles' own, widened a
    // little, so a formula that drifts is caught without the test needing to be rewritten every
    // time a constant is refined.
    let expected = [
        ("Bt: American IPA", 1.056..1.075, 5.5..7.5, 5.0..9.0, 40.0..80.0),
        ("Bt: American Pale Ale", 1.045..1.060, 4.0..6.0, 5.0..11.0, 30.0..55.0),
        ("Bt: Belgian Blonde Ale", 1.060..1.075, 6.0..7.5, 4.0..8.0, 15.0..35.0),
    ];

    for (name, gravity, alcohol, colour, bitterness) in expected {
        let id = recipe_named(&app, name).await;
        let (status, figures) = request(&app, "GET", &format!("/api/recipes/{id}/calculated"), None).await;
        assert_eq!(status, StatusCode::OK, "{figures}");

        let number = |key: &str| figures[key].as_f64().unwrap_or_else(|| panic!("{name} has no {key}"));
        assert!(gravity.contains(&number("og")), "{name}: original gravity {}", number("og"));
        assert!(alcohol.contains(&number("abv_pct")), "{name}: alcohol {}%", number("abv_pct"));
        assert!(colour.contains(&number("color_srm")), "{name}: colour {} SRM", number("color_srm"));
        assert!(bitterness.contains(&number("ibus")), "{name}: bitterness {}", number("ibus"));
        // Fermentation lowers the gravity, and a beer that finishes where it started has not
        // been fermented by anything.
        assert!(number("fg") < number("og"), "{name} did not ferment");
    }
}

/// BeerXML records a mash step's water as an `INFUSE_AMOUNT` or a `DECOCTION_AMOUNT` depending on
/// what kind of step it is, and neither is the column the database keeps it in.  Losing it makes
/// the wort coming off the mash come out negative, because the grain still absorbs its share.
#[tokio::test]
async fn a_mash_step_keeps_the_water_it_calls_for() {
    let app = library("mash").await;
    let id = recipe_named(&app, "Bt: American IPA").await;

    let (status, recipe) = request(&app, "GET", &format!("/api/recipes/{id}/full"), None).await;
    assert_eq!(status, StatusCode::OK);
    let steps = recipe["mash"][1].as_array().expect("the mash steps");
    assert!(!steps.is_empty(), "an all-grain recipe has a mash");
    for step in steps {
        let amount = step["amount_l"].as_f64().unwrap_or_default();
        assert!(amount > 0.0, "a mash step with no water: {step}");
    }

    let (_, figures) = request(&app, "GET", &format!("/api/recipes/{id}/calculated"), None).await;
    let wort = figures["wort_from_mash_l"].as_f64().expect("wort from the mash");
    assert!(wort > 0.0, "the mash produced {wort} litres of wort");
}

/// Everything that went in comes back out: the library exports and re-imports into an empty
/// database with the same number of rows in every table.
#[tokio::test]
async fn the_library_survives_a_trip_out_and_back() {
    let app = library("round-trip").await;
    let (status, exported) = request(&app, "GET", "/api/export", None).await;
    assert_eq!(status, StatusCode::OK);

    let empty = support::app("library-round-trip").await;
    let text = serde_json::to_string(&exported).expect("encode");
    let (status, summary) = request(&empty, "POST", "/api/import", support::text(&text)).await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    assert_eq!(summary["format"], "dot_beer");

    for table in ["recipe", "hop", "fermentable", "yeast", "style", "mash", "equipment"] {
        let count = |document: &Value| document.as_array().map(Vec::len).unwrap_or_default();
        let (_, before) = request(&app, "GET", &format!("/api/{table}"), None).await;
        let (_, after) = request(&empty, "GET", &format!("/api/{table}"), None).await;
        assert_eq!(count(&before), count(&after), "{table} did not survive the round trip");
    }
}
