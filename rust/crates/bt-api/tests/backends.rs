// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/tests/backends.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The same API, driven against PostgreSQL and MariaDB.
//!
//! Everything else in this crate runs on SQLite, which is the default and the one that needs no
//! server.  These run the same requests against the other two, because the places they differ --
//! how a boolean is stored, how placeholders are numbered, whether a schema and a database are
//! the same thing -- are exactly the places a port goes wrong.
//!
//! Skipped unless `BT_TEST_POSTGRES_URL` or `BT_TEST_MYSQL_URL` is set, so neither CI nor a
//! developer machine needs a server to run `cargo test`.
//!
//! Each test makes its own namespace, works only inside it, and drops it at the end, so pointing
//! these at an existing development server leaves nothing behind.

mod support;

use axum::http::StatusCode;
use bt_api::{AppState, api};
use bt_schema::Namespace;
use bt_store::Store;
use serde_json::{Value, json};
use support::{json as body, repository, request};

/// A namespace name that is this process's alone, so two runs do not collide.
fn namespace_name() -> String {
    format!("bt_api_test_{}", std::process::id())
}

/// Connect, make a namespace, and hand back a router that works inside it.
///
/// PostgreSQL takes the namespace in the connection string rather than with a `SET` afterwards: a
/// pooled connection that was told once could later be handed to anything, which is the shape of
/// the worst bug this system could have (`PLAN.md` Appendix B).  MariaDB has no schemas, so a
/// namespace is a database and it goes in the path of the URL.
async fn connect(url: &str) -> (axum::Router, Store, Namespace) {
    support::show_logs();
    let namespace = Namespace::named(namespace_name()).expect("a usable namespace name");
    let Namespace::Named(name) = &namespace else { unreachable!("just built as named") };

    // One connection makes the namespace; another works inside it, because which namespace a
    // connection is in is settled when it is opened.
    let administrative = Store::connect(url).await.expect("connect");
    // A run that failed before it could tidy up leaves its namespace behind, and reusing one
    // would mean testing against tables made by an older build.  Starting by removing it also
    // means a failure here leaves at most one namespace rather than one per run.
    administrative.drop_namespace(&namespace).await.expect("clear any namespace left behind");
    administrative.create_namespace(&namespace).await.expect("make the namespace");

    let scoped = if url.starts_with("postgres") {
        format!("{url}?options=-c%20search_path%3D{name}")
    } else {
        let base = url.trim_end_matches('/');
        let without_database = base.rsplit_once('/').map(|(head, _)| head).unwrap_or(base);
        format!("{without_database}/{name}")
    };
    let store = Store::connect(&scoped).await.expect("connect to the namespace");
    store.prepare(&Namespace::Default).await.expect("bring the schema up inside it");
    let app = api().with_state(AppState::new(store, repository().join("data")));
    (app, administrative, namespace)
}

/// Everything the API does, against one backend.
async fn exercise(app: &axum::Router) {
    // A row goes in, comes back, changes and goes away.
    let (status, hop) = request(app, "POST", "/api/hop", body(json!({"name": "Fuggle", "alpha_pct": 4.5}))).await;
    assert_eq!(status, StatusCode::OK, "{hop}");
    let hop_id = hop["id"].as_i64().expect("an id");

    let (status, read) = request(app, "GET", &format!("/api/hop/{hop_id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["alpha_pct"], 4.5);

    // A boolean: SQLite has none, PostgreSQL has a real one and MariaDB calls it a TINYINT.
    // Round-tripping one is the single most backend-dependent thing the store does.
    let (status, updated) =
        request(app, "PUT", &format!("/api/hop/{hop_id}"), body(json!({"name": "Fuggle", "deleted": false}))).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["deleted"], false, "a false is not a zero");

    // A whole file, which exercises folders, foreign keys and every table the reader touches.
    let (status, summary) = request(app, "POST", "/api/import", support::text(RECIPE)).await;
    assert_eq!(status, StatusCode::OK, "{summary}");
    let recipe_id = summary["recipe_ids"][0].as_i64().expect("a recipe");

    let (status, figures) = request(app, "GET", &format!("/api/recipes/{recipe_id}/calculated"), None).await;
    assert_eq!(status, StatusCode::OK, "{figures}");
    let og = figures["og"].as_f64().expect("original gravity");
    assert!((1.040..1.070).contains(&og), "original gravity was {og}");

    // ...and back out again, which reads every row through the untyped path.
    let (status, exported) = request(app, "GET", "/api/export", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(contains_name(&exported, "Backend Bitter"), "the recipe did not come back out");

    let (status, _) = request(app, "DELETE", &format!("/api/hop/{hop_id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// Whether a name appears anywhere in a document.
fn contains_name(value: &Value, name: &str) -> bool {
    match value {
        Value::Object(fields) => {
            fields.get("name").and_then(Value::as_str) == Some(name)
                || fields.values().any(|nested| contains_name(nested, name))
        }
        Value::Array(items) => items.iter().any(|item| contains_name(item, name)),
        _ => false,
    }
}

const RECIPE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<RECIPES>
  <RECIPE>
    <NAME>Backend Bitter</NAME>
    <VERSION>1</VERSION>
    <TYPE>All Grain</TYPE>
    <FOLDER_PATH>/Test</FOLDER_PATH>
    <BATCH_SIZE>20.0</BATCH_SIZE>
    <BOIL_SIZE>25.0</BOIL_SIZE>
    <BOIL_TIME>60.0</BOIL_TIME>
    <EFFICIENCY>75.0</EFFICIENCY>
    <FERMENTABLES>
      <FERMENTABLE>
        <NAME>Pale Malt</NAME><VERSION>1</VERSION><TYPE>Grain</TYPE>
        <AMOUNT>4.0</AMOUNT><YIELD>80.0</YIELD><COLOR>3.0</COLOR>
      </FERMENTABLE>
    </FERMENTABLES>
    <HOPS>
      <HOP>
        <NAME>Fuggle</NAME><VERSION>1</VERSION><ALPHA>4.5</ALPHA>
        <AMOUNT>0.050</AMOUNT><USE>Boil</USE><TIME>60.0</TIME>
      </HOP>
    </HOPS>
    <YEASTS>
      <YEAST>
        <NAME>London Ale</NAME><VERSION>1</VERSION><TYPE>Ale</TYPE><FORM>Liquid</FORM>
        <AMOUNT>0.035</AMOUNT><ATTENUATION>73.0</ATTENUATION>
      </YEAST>
    </YEASTS>
  </RECIPE>
</RECIPES>
"#;

#[tokio::test]
async fn the_api_works_against_postgresql() {
    let Ok(url) = std::env::var("BT_TEST_POSTGRES_URL") else {
        eprintln!("skipping: BT_TEST_POSTGRES_URL is not set");
        return;
    };
    let (app, administrative, namespace) = connect(&url).await;
    exercise(&app).await;
    // Leave nothing behind, so this is safe to point at a development server.
    administrative.drop_namespace(&namespace).await.expect("drop the namespace");
}

#[tokio::test]
async fn the_api_works_against_mariadb() {
    let Ok(url) = std::env::var("BT_TEST_MYSQL_URL") else {
        eprintln!("skipping: BT_TEST_MYSQL_URL is not set");
        return;
    };
    let (app, administrative, namespace) = connect(&url).await;
    exercise(&app).await;
    // Leave nothing behind, so this is safe to point at a development server.
    administrative.drop_namespace(&namespace).await.expect("drop the namespace");
}
