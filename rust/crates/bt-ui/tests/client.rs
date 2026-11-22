// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-ui/tests/client.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! The interface's client, against the real server.
//!
//! A stand-in server would agree with whatever it was told to say, and the risk worth covering is
//! exactly that the two sides disagree -- a route renamed, a field spelled differently, a status
//! code read the wrong way round.  So this starts `bt-api` on a real socket and talks to it.

use std::net::{Ipv4Addr, SocketAddr};

use bt_api::{AppState, api};
use bt_model::tables::{FermentableRow, HopRow};
use bt_schema::Namespace;
use bt_store::Store;
use bt_ui::Client;

/// Start a server on a port the operating system picks, and return a client pointed at it.
///
/// Port zero means "any free one", which is what lets these run at the same time as each other
/// and as anything else on the machine.
async fn serving(label: &str) -> Client {
    let path = std::env::temp_dir().join(format!("bt-ui-{label}.sqlite"));
    let _ = std::fs::remove_file(&path);
    std::fs::File::create(&path).expect("create the database file");

    let store = Store::connect(&format!("sqlite://{}", path.display())).await.expect("connect");
    store.prepare(&Namespace::Default).await.expect("prepare");
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data");
    let app = api().with_state(AppState::new(store, data));

    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.expect("bind");
    let address = listener.local_addr().expect("the address it got");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Client::new(format!("http://{address}"))
}

/// As [`serving`], but with the content that ships with the application already in it.
async fn serving_with_content(label: &str) -> Client {
    let path = std::env::temp_dir().join(format!("bt-ui-{label}.sqlite"));
    let _ = std::fs::remove_file(&path);
    std::fs::File::create(&path).expect("create the database file");

    let store = Store::connect(&format!("sqlite://{}", path.display())).await.expect("connect");
    store.prepare(&Namespace::Default).await.expect("prepare");
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../data");
    bt_store::default_content::load(&store, &data).await.expect("load the shipped content");
    let app = api().with_state(AppState::new(store, data));

    let listener = tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await.expect("bind");
    let address = listener.local_addr().expect("the address it got");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Client::new(format!("http://{address}"))
}

#[tokio::test]
async fn a_row_can_be_stored_read_back_changed_and_removed() {
    let client = serving("crud").await;

    let stored = client
        .create_hop(&HopRow { name: Some("Fuggle".to_owned()), alpha_pct: Some(4.5), ..HopRow::default() })
        .await
        .expect("store a hop");
    assert!(stored.id > 0, "the database assigns the key");
    assert_eq!(stored.name.as_deref(), Some("Fuggle"));

    let read = client.hop(stored.id).await.expect("read it back");
    assert_eq!(read, stored);

    let changed =
        client.update_hop(stored.id, &HopRow { alpha_pct: Some(5.25), ..stored.clone() }).await.expect("change it");
    assert_eq!(changed.alpha_pct, Some(5.25));

    assert_eq!(client.hops().await.expect("list them").len(), 1);

    client.delete_hop(stored.id).await.expect("delete it");
    // Deleting is soft, so the row is still there and still readable -- it is the catalogue
    // that stops showing it.
    assert_eq!(client.hop(stored.id).await.expect("still readable").deleted, Some(true));
}

/// A refusal has to arrive as a refusal, with the server's own words, rather than as an empty
/// list or a default-constructed row.
#[tokio::test]
async fn asking_for_something_absent_is_an_error_not_an_empty_answer() {
    let client = serving("missing").await;

    let failure = client.hop(9999).await.expect_err("there is no hop 9999");
    assert!(failure.is_not_found(), "{failure:?}");
    assert!(failure.to_string().contains("9999"), "{failure}");
}

/// Nothing stored, nothing listed -- and that is not an error.
#[tokio::test]
async fn an_empty_catalogue_is_an_empty_list() {
    let client = serving("empty").await;
    assert!(client.fermentables().await.expect("list").is_empty());
}

/// A server that is not there is reported as such, rather than as an empty catalogue.  The two
/// look identical in a table and mean entirely different things.
#[tokio::test]
async fn an_unreachable_server_is_not_an_empty_catalogue() {
    // Port 1 on loopback: nothing listens there, and nothing may bind it without privilege.
    let client = Client::new("http://127.0.0.1:1");
    let failure = client.hops().await.expect_err("nothing is listening");
    assert!(!failure.is_not_found(), "{failure:?}");
    assert!(matches!(failure, bt_ui::ApiError::Unreachable(_)), "{failure:?}");
}

/// The client reads the fields the catalogue shows, under the names the server sends them by.
#[tokio::test]
async fn the_columns_the_catalogue_shows_survive_the_trip() {
    let client = serving("columns").await;

    let stored = client
        .create_fermentable(&FermentableRow {
            name: Some("Maris Otter".to_owned()),
            ftype: Some("grain".to_owned()),
            fine_grind_yield_pct: Some(81.5),
            color_lovibond: Some(3.0),
            ..FermentableRow::default()
        })
        .await
        .expect("store a malt");

    let read = client.fermentable(stored.id).await.expect("read it back");
    assert_eq!(read.name.as_deref(), Some("Maris Otter"));
    assert_eq!(read.fine_grind_yield_pct, Some(81.5));
    assert_eq!(read.color_lovibond, Some(3.0));
    // The enumerated column parses rather than arriving as a string the interface cannot read.
    assert_eq!(read.ftype().expect("a known type").map(|value| value.as_str()), Some("grain"));
}

/// Two figures that came the same way out of the same arithmetic.
///
/// Not exact equality, and not because the arithmetic differs: `serde_json` does not round-trip
/// every `f64`.  192.61098795228318 is written out in full and read back as 192.6109879522832,
/// one unit in the last place away.  That is a property of the transport, so the comparison is
/// loose enough to ignore it and still some nine orders of magnitude tighter than any real
/// disagreement -- the mash bug this test first caught moved a final gravity by half a percent.
#[track_caller]
fn agree(here: f64, there: f64, what: &str) {
    let difference = (here - there).abs();
    let scale = here.abs().max(there.abs()).max(1.0);
    assert!(difference / scale < 1e-12, "{what}: the interface says {here}, the server says {there}");
}

/// The figures worked out in the interface are the figures the server works out.
///
/// This is the architectural claim the whole choice of Rust for the interface rests on: that
/// `bt-calc` running in a browser and `bt-calc` running in the server are the same code giving
/// the same answers, so a recipe being edited can be costed without a round trip.  If the two
/// ever disagree -- a default changed on one side, a preference read from somewhere -- the panel
/// would quietly show something the server would not.
#[tokio::test]
async fn the_interface_and_the_server_agree_on_what_a_recipe_comes_to() {
    let client = serving_with_content("agreement").await;

    let recipes = client.recipe_rows().await.expect("the recipes");
    assert!(recipes.len() > 20, "the shipped content has a library of recipes in it");

    // Several recipes rather than one: they differ in whether they have equipment, a mash, a
    // boil, non-fermentable sugar, and each of those is a branch in the arithmetic.
    for row in recipes.iter().take(8) {
        let recipe = client.recipe(row.id).await.expect("the recipe in full");
        let here = bt_calc::recipe::calculate_recipe(&recipe, Default::default(), Default::default());
        let there = client.calculated(row.id).await.expect("what the server makes of it");

        let name = row.name.as_deref().unwrap_or("?");
        agree(here.og, there.og, &format!("{name}: original gravity"));
        agree(here.fg, there.fg, &format!("{name}: final gravity"));
        agree(here.abv_pct, there.abv_pct, &format!("{name}: alcohol"));
        agree(here.ibus, there.ibus, &format!("{name}: bitterness"));
        agree(here.color_srm, there.color_srm, &format!("{name}: colour"));
        agree(here.final_volume_l, there.final_volume_l, &format!("{name}: volume into the fermenter"));

        assert_eq!(
            here.ibus_per_addition.len(),
            there.ibus_per_addition.len(),
            "{name}: a different number of hop additions"
        );
        for (index, (here, there)) in here.ibus_per_addition.iter().zip(&there.ibus_per_addition).enumerate() {
            agree(*here, *there, &format!("{name}: bitterness from hop addition {index}"));
        }
    }
}

/// The recipe screen reads a recipe's style, equipment and additions off the aggregate rather
/// than fetching each of them, so the aggregate has to actually carry them.
#[tokio::test]
async fn a_recipe_arrives_with_everything_the_screen_shows() {
    let client = serving_with_content("aggregate").await;

    let recipes = client.recipe_rows().await.expect("the recipes");
    let id = recipes.iter().find(|row| row.name.as_deref() == Some("Bt: American IPA")).expect("the shipped IPA").id;
    let recipe = client.recipe(id).await.expect("the recipe in full");

    assert_eq!(recipe.row.name.as_deref(), Some("Bt: American IPA"));
    assert!(recipe.style.is_some(), "the style is resolved, not left as a key");
    assert!(recipe.equipment.is_some(), "so is the equipment");
    assert!(!recipe.fermentables.is_empty(), "an all-grain recipe has malt in it");
    assert!(!recipe.hops.is_empty(), "an IPA has hops in it");
    assert!(!recipe.yeasts.is_empty(), "and something to ferment it");

    // Every addition names the ingredient it is an addition of; a tab showing blank names
    // would mean the join had not been made.
    for addition in &recipe.hops {
        assert!(addition.ingredient.is_some(), "a hop addition with no hop");
    }
    // And an amount, which is what the Amount column reads.
    use bt_model::recipe::IngredientAmount;
    assert!(recipe.fermentables.iter().all(|addition| addition.addition.amount().is_some()));
}

/// The address the interface uses when nothing has told it one must be usable.
///
/// This is the bug that shipped: the base was empty, meaning "the page's own origin", but
/// `reqwest` builds on `Url` and `Url` cannot parse `/api/hop` without one.  Every request in
/// the browser failed with "builder error", which names neither the URL nor the problem.
#[test]
fn the_default_address_is_one_a_request_can_be_built_from() {
    let client = Client::default();
    let base = client.base();
    assert!(
        base.starts_with("http://") || base.starts_with("https://"),
        "the default address {base:?} has no scheme, so no request can be built from it"
    );
    assert!(!base.ends_with('/'), "a trailing slash would double up against the path: {base:?}");
}

/// An address that cannot be used is reported as such, naming what was wrong with it, rather
/// than surfacing whatever `reqwest` says about failing to build a request.
#[tokio::test]
async fn an_unusable_address_says_what_is_wrong_with_it() {
    for base in ["", "/", "127.0.0.1:8080", "localhost"] {
        let failure = Client::new(base).hops().await.expect_err("{base} is not an address");
        let message = failure.to_string();
        assert!(matches!(failure, bt_ui::ApiError::Unreachable(_)), "{base:?}: {failure:?}");
        assert!(message.contains("scheme"), "{base:?} should say what it is missing, said {message:?}");
    }
}

/// A request that gets nowhere names the address it tried, so the next failure of this shape
/// is one line to diagnose rather than a guess.
#[tokio::test]
async fn a_failed_request_names_the_address_it_tried() {
    let failure = Client::new("http://127.0.0.1:1").hops().await.expect_err("nothing is listening");
    assert!(failure.to_string().contains("http://127.0.0.1:1/api/hop"), "{failure}");
}

/// Catalogue names appear in addresses people keep, so they have to survive being written down
/// and read back, and nothing may share a name with anything else.
#[test]
fn every_catalogue_has_an_address_that_round_trips() {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use bt_ui::views::Which;

    let mut seen = BTreeSet::new();
    for which in Which::ALL {
        let slug = which.slug();
        assert!(seen.insert(slug), "two catalogues are both called {slug:?}");
        assert_eq!(Which::from_str(slug).expect("reads back"), which);
        assert_eq!(slug, slug.to_lowercase(), "{slug:?} should be lower case");
        assert!(
            slug.chars().all(|character| character.is_ascii_lowercase()),
            "{slug:?} should be plain letters, so that it needs no escaping"
        );
    }
    assert!(Which::from_str("sausages").is_err(), "an address naming nothing is an error");
}

/// The editor works in columns rather than in typed structs, because it is built from the
/// schema and cannot know the type at compile time.  That path has to store what it is given
/// and give back what it stored.
#[tokio::test]
async fn a_row_can_be_made_and_changed_through_the_untyped_path() {
    use serde_json::{Map, Value, json};

    let client = serving("untyped").await;

    let mut row = Map::new();
    row.insert("name".to_owned(), json!("Citra"));
    row.insert("alpha_pct".to_owned(), json!(12.0));
    row.insert("form".to_owned(), json!("pellet"));

    let stored = client.post_row("hop", &row).await.expect("store it");
    let id = stored["id"].as_i64().expect("a key") as i32;
    assert_eq!(stored["name"], "Citra");
    assert_eq!(stored["alpha_pct"], 12.0);

    // An empty box clears a column rather than writing a zero or an empty string into it:
    // almost every column in this schema is nullable, and most of them are null.
    let mut changed = stored.clone();
    changed.insert("alpha_pct".to_owned(), Value::Null);
    changed.insert("notes".to_owned(), json!("tastes of grapefruit"));
    let after = client.put_row("hop", id, &changed).await.expect("change it");
    assert_eq!(after["alpha_pct"], Value::Null, "clearing a column should clear it");
    assert_eq!(after["notes"], "tastes of grapefruit");
    assert_eq!(after["form"], "pellet", "a column nobody touched should be left alone");

    let read = client.row("hop", id).await.expect("read it back");
    assert_eq!(read, after);
}

/// Every table has an editor, and every editor has every column of its table that is worth
/// editing.  This is the point of building them from the schema: a new column cannot appear
/// without a way to edit it appearing too.
#[test]
fn every_table_has_an_editor_covering_its_columns() {
    use bt_model::tables::{TABLES, columns_of, editable_columns};

    for table in TABLES {
        let editable = editable_columns(table);
        assert!(!editable.is_empty(), "{table} has no editable columns");

        let columns = columns_of(table).expect("a known table");
        for field in editable {
            assert!(columns.contains(&field.column), "{table}.{} is not a column of it", field.column);
            assert!(!field.label.is_empty(), "{table}.{} has no label", field.column);
        }

        // The key is the database's to assign and the foreign keys name other rows, so neither
        // is offered; everything else is.
        assert!(!editable.iter().any(|field| field.column == "id"), "{table} offers its primary key for editing");
    }
}

/// An enumerated column offers what may go in it, by the name it is shown by.
#[test]
fn an_enumerated_column_offers_the_values_it_accepts() {
    use bt_model::tables::editable_columns;

    let ftype =
        editable_columns("fermentable").iter().find(|field| field.column == "ftype").expect("fermentables have a type");
    assert_eq!(ftype.label, "Type");
    assert!(ftype.choices.contains(&("dry extract", "Dry Extract")), "{:?}", ftype.choices);

    // What is stored and what is shown are different strings, and both are needed: one goes
    // into the column, the other onto the screen.
    for (stored, shown) in ftype.choices {
        assert!(!stored.is_empty() && !shown.is_empty());
    }

    // A column that is not enumerated offers nothing, rather than an empty dropdown.
    let alpha = editable_columns("hop").iter().find(|field| field.column == "alpha_pct").expect("hops have alpha");
    assert!(alpha.choices.is_empty());
}

/// Deleting through the untyped path the editor uses.
#[tokio::test]
async fn a_row_can_be_deleted_through_the_untyped_path() {
    use serde_json::{Map, json};

    let client = serving("untyped-delete").await;
    let mut row = Map::new();
    row.insert("name".to_owned(), json!("Doomed"));
    let stored = client.post_row("hop", &row).await.expect("store it");
    let id = stored["id"].as_i64().expect("a key") as i32;

    client.delete_row("hop", id).await.expect("delete it");

    // Soft, because `hop` records it: the row stays so that recipes using it still resolve,
    // and the catalogue stops listing it.
    let after = client.row("hop", id).await.expect("still readable");
    assert_eq!(after["deleted"], true);
}

/// A whole library goes in through the same route the interface uses, and comes back out.
#[tokio::test]
async fn a_file_can_be_imported_and_the_library_exported() {
    let client = serving("transfer").await;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../data/DefaultContent006-MashAndWaterProfiles.beer");
    let document = std::fs::read_to_string(&path).expect("the shipped profiles");
    let summary = client.import(&document).await.expect("import it");
    assert_eq!(summary.format, "dot_beer", "the format is worked out from the contents");
    assert!(summary.rows_written > 0, "nothing was stored");

    // What went in is there.
    assert!(!client.mashes().await.expect("the mashes").is_empty());

    // The export address is one a browser can be pointed at, and the server answers it.
    for format in ["dot_beer", "beer_json", "beer_xml"] {
        let url = client.export_url(format);
        assert!(url.ends_with(&format!("/api/export?format={format}")), "{url}");
        let response = reqwest::get(&url).await.expect("fetch the export");
        assert!(response.status().is_success(), "{format}: {}", response.status());
        assert!(!response.text().await.expect("read it").is_empty(), "{format} exported nothing");
    }
}

/// A file that is not any of the three formats is refused, and says so.
#[tokio::test]
async fn something_that_is_not_a_brewing_file_is_refused() {
    let client = serving("transfer-nonsense").await;
    let failure = client.import("this is not a recipe").await.expect_err("not a brewing file");
    assert!(!failure.is_not_found(), "{failure:?}");
    assert!(failure.to_string().contains("not a"), "{failure}");
}
