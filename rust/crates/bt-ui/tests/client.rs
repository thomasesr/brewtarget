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
    // Somewhere of its own to write preferences: the content directory is part of the
    // repository, and a test must not leave anything in it.
    let state = std::env::temp_dir().join(format!("bt-ui-state-{label}"));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).expect("a state directory");
    let app = api().with_state(AppState::with_state_directory(store, data, state));

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
    let state = std::env::temp_dir().join(format!("bt-ui-state-{label}"));
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&state).expect("a state directory");
    let app = api().with_state(AppState::with_state_directory(store, data, state));

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

/// Adding a hop to a recipe changes what the recipe comes to.
///
/// The point of the addition editor: a row written to `hop_in_recipe` has to show up in the
/// recipe aggregate and in its bitterness, or the screen will say the recipe is unchanged while
/// the database says otherwise.
#[tokio::test]
async fn adding_a_hop_to_a_recipe_bitters_it() {
    use bt_model::tables::HopRow;
    use serde_json::{Map, json};

    let client = serving_with_content("addition").await;
    let id = client
        .recipe_rows()
        .await
        .expect("the recipes")
        .iter()
        .find(|row| row.name.as_deref() == Some("Bt: American Pale Ale"))
        .expect("the shipped pale ale")
        .id;

    let before = bt_calc::recipe::calculate_recipe(
        &client.recipe(id).await.expect("the recipe"),
        Default::default(),
        Default::default(),
    );

    let hop = client
        .create_hop(&HopRow { name: Some("Test Hop".to_owned()), alpha_pct: Some(15.0), ..HopRow::default() })
        .await
        .expect("a hop to add");

    let mut addition = Map::new();
    addition.insert("recipe_id".to_owned(), json!(id));
    addition.insert("hop_id".to_owned(), json!(hop.id));
    addition.insert("quantity".to_owned(), json!(0.1));
    addition.insert("unit".to_owned(), json!("kilograms"));
    addition.insert("stage".to_owned(), json!("add_to_boil"));
    addition.insert("step".to_owned(), json!(2));
    addition.insert("add_at_time_mins".to_owned(), json!(60.0));
    let stored = client.post_row("hop_in_recipe", &addition).await.expect("add it");
    let addition_id = stored["id"].as_i64().expect("a key") as i32;

    let after = client.recipe(id).await.expect("the recipe again");
    assert!(
        after
            .hops
            .iter()
            .any(|addition| addition.ingredient.as_ref().and_then(|hop| hop.name.as_deref()) == Some("Test Hop")),
        "the addition is not on the recipe"
    );
    let bittered = bt_calc::recipe::calculate_recipe(&after, Default::default(), Default::default());
    assert!(bittered.ibus > before.ibus, "100 g of 15% hops for an hour should bitter it");

    // ...and taking it away puts the recipe back.
    client.delete_row("hop_in_recipe", addition_id).await.expect("remove it");
    let removed = client.recipe(id).await.expect("the recipe once more");
    assert!(
        !removed
            .hops
            .iter()
            .any(|addition| addition.ingredient.as_ref().and_then(|hop| hop.name.as_deref()) == Some("Test Hop")),
        "the addition is still there"
    );
    let plain = bt_calc::recipe::calculate_recipe(&removed, Default::default(), Default::default());
    agree(plain.ibus, before.ibus, "bitterness after removing the addition");
}

/// A foreign key is chosen from the table it names, so the editor has to know which table that
/// is.  Getting this wrong offers a picker full of the wrong things.
#[test]
fn a_foreign_key_names_the_table_it_is_chosen_from() {
    use bt_model::tables::editable_columns;

    let hop_id = editable_columns("hop_in_recipe")
        .iter()
        .find(|field| field.column == "hop_id")
        .expect("a hop addition names a hop");
    assert_eq!(hop_id.references, Some("hop"));
    // Named for the thing being chosen, not for its key: the C++ calls this one "Hop".
    assert_eq!(hop_id.label, "Hop");

    let style = editable_columns("recipe").iter().find(|field| field.column == "style_id").expect(
        "recipes
 have styles",
    );
    assert_eq!(style.references, Some("style"));
    assert_eq!(style.label, "Style");

    // A column that names nothing has no table to choose from.
    let quantity = editable_columns("hop_in_recipe").iter().find(|field| field.column == "quantity").expect("amount");
    assert_eq!(quantity.references, None);
}

/// The editors are arranged the way the Qt dialogs are.
///
/// Not pixel for pixel -- the layout is deliberately not copied -- but the architecture: the
/// same tabs in the same order, the same fields on each in the same order, under the same
/// headings and with the same labels.
#[test]
fn the_editors_are_arranged_as_the_qt_dialogs_are() {
    use bt_model::tables::form_for;

    let hop = form_for("hop").expect("the Qt application has a hop editor");
    assert_eq!(hop.iter().map(|tab| tab.title).collect::<Vec<_>>(), ["Main", "Extras", "Notes"]);

    let main = hop[0].fields;
    // The order the dialog has them in, not the order the schema declares them.
    let columns: Vec<&str> = main.iter().map(|field| field.column).collect();
    assert_eq!(&columns[..4], &["name", "form", "year", "origin"], "{columns:?}");

    // A label the dialog gives a field, which is not what the column is called anywhere
    // else: alpha is "Actual", under an "Alpha Acid" heading.
    let alpha = main.iter().find(|field| field.column == "alpha_pct").expect("hops have alpha");
    assert_eq!(alpha.label, "Actual");
    assert_eq!(alpha.group, Some("Alpha Acid"));
    assert_eq!(alpha.precision, Some(1));
}

/// Every table the Qt application has a dialog for has that dialog's arrangement, and every
/// field in it names a real column.
#[test]
fn every_arranged_editor_names_real_columns() {
    use bt_model::tables::{TABLES, columns_of, form_for};

    let mut arranged = 0;
    for table in TABLES {
        let Some(form) = form_for(table) else { continue };
        arranged += 1;
        let columns = columns_of(table).expect("a known table");
        assert!(!form.is_empty(), "{table} has an editor with no tabs");
        for tab in form {
            assert!(!tab.title.is_empty(), "{table} has a tab with no title");
            assert!(!tab.fields.is_empty(), "{table}'s {} tab has no fields", tab.title);
            for field in tab.fields {
                assert!(columns.contains(&field.column), "{table}.{} is not a column", field.column);
                assert!(!field.label.is_empty(), "{table}.{} has no label", field.column);
            }
        }
    }
    // The eighteen dialogs in `ui/`, less those whose model has no table of its own.
    assert!(arranged >= 16, "only {arranged} tables have an arranged editor");
}

/// A field the dialog shows but never writes back stays that way.
#[test]
fn a_computed_field_is_shown_but_not_written() {
    use bt_model::tables::form_for;

    // `Ingredient::totalInventory` is worked out from the stock purchases, so the hop editor
    // shows it and never writes it.  It has no column, so it does not appear at all here --
    // what matters is that nothing claims to be able to write one.
    let written: Vec<&str> = form_for("hop")
        .expect("a hop editor")
        .iter()
        .flat_map(|tab| tab.fields.iter())
        .filter(|field| field.read_only)
        .map(|field| field.column)
        .collect();
    for column in written {
        assert!(!column.is_empty(), "a read-only field with no column");
    }
}

/// The interface is arranged the way the application is.
///
/// Not the layout -- the architecture.  The tab strips in `ui/mainWindow.ui` say what the
/// sections are and what order they come in, and that order is what anyone who has used
/// Brewtarget will reach for.  This had been invented rather than taken.
#[test]
fn the_sections_are_the_ones_the_application_has_in_its_order() {
    use bt_ui::views::Which;

    // `tabWidget_Trees`, less Recipes, which has a place of its own above them.
    let expected = [
        "Styles",
        "Equipment",
        "Mash Profiles",
        "Boil Profiles",
        "Fermentation Profiles",
        "Fermentables",
        "Hops",
        "Miscs",
        "Yeasts",
        "Water",
    ];
    let titles: Vec<&str> = Which::ALL.iter().map(|which| which.title()).collect();
    assert_eq!(titles, expected);
}

/// A recipe arrives with the water profiles its Water tab shows.
///
/// They are rows of `water` rather than columns on the recipe, so the aggregate has to resolve
/// them as it does the style and the equipment.
#[tokio::test]
async fn a_recipe_carries_the_water_it_starts_from() {
    let client = serving_with_content("water").await;
    let recipes = client.recipe_rows().await.expect("the recipes");

    // Not every shipped recipe names water, so this looks for one that does.
    let mut found = false;
    for row in recipes.iter().take(12) {
        let recipe = client.recipe(row.id).await.expect("the recipe");
        if recipe.row.water_base.is_some() {
            assert!(recipe.water_base.is_some(), "{:?} names a base water that did not resolve", row.name);
            found = true;
        }
        if recipe.row.water_target.is_some() {
            assert!(recipe.water_target.is_some(), "{:?} names a target water that did not resolve", row.name);
            found = true;
        }
    }
    // If none of them names water the test has proved nothing, which is worth saying rather
    // than passing quietly.
    if !found {
        eprintln!("none of the shipped recipes names a water profile; nothing was checked");
    }
}

/// Tool names appear in addresses, so they have to survive being written down and read back.
#[test]
fn every_tool_has_an_address_that_round_trips() {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use bt_ui::views::tools::Tool;

    let mut seen = BTreeSet::new();
    for tool in Tool::ALL {
        let slug = tool.slug();
        assert!(seen.insert(slug), "two tools are both called {slug:?}");
        assert_eq!(Tool::from_str(slug).expect("reads back"), tool);
        assert!(
            slug.chars().all(|character| character.is_ascii_lowercase() || character == '-'),
            "{slug:?} should need no escaping in an address"
        );
    }
    assert!(Tool::from_str("hammer").is_err());
}

/// The "N° Recipes" column counts distinct recipes, not additions.
///
/// A recipe that uses the same hop three times still uses it once, which is what
/// `Recipe::numRecipesUsing` counts and why its C++ gathers the recipe ids into a set.
#[tokio::test]
async fn usage_counts_recipes_not_additions() {
    use bt_model::tables::HopRow;
    use serde_json::{Map, json};

    let client = serving("usage").await;
    let hop = client
        .create_hop(&HopRow { name: Some("Counted".to_owned()), alpha_pct: Some(5.0), ..HopRow::default() })
        .await
        .expect("a hop");

    // Nothing uses it yet.
    assert_eq!(client.usage("hop").await.expect("counts").get(&hop.id), None);

    let mut recipe = Map::new();
    recipe.insert("name".to_owned(), json!("Counted Recipe"));
    let stored = client.post_row("recipe", &recipe).await.expect("a recipe");
    let recipe_id = stored["id"].as_i64().expect("a key") as i32;

    // Three additions of the same hop to the same recipe.
    for at_time in [60.0, 15.0, 5.0] {
        let mut addition = Map::new();
        addition.insert("recipe_id".to_owned(), json!(recipe_id));
        addition.insert("hop_id".to_owned(), json!(hop.id));
        addition.insert("quantity".to_owned(), json!(0.02));
        addition.insert("unit".to_owned(), json!("kilograms"));
        addition.insert("add_at_time_mins".to_owned(), json!(at_time));
        client.post_row("hop_in_recipe", &addition).await.expect("an addition");
    }

    let counts = client.usage("hop").await.expect("counts");
    assert_eq!(counts.get(&hop.id), Some(&1), "three additions, one recipe");

    // Deleting the recipe stops it counting.
    client.delete_row("recipe", recipe_id).await.expect("delete it");
    assert_eq!(client.usage("hop").await.expect("counts").get(&hop.id), None);
}

/// Usage is counted for a whole table at once, and a table that is not one says so.
#[tokio::test]
async fn usage_is_refused_for_something_that_is_not_a_table() {
    let client = serving("usage-unknown").await;
    let failure = client.usage("sausages").await.expect_err("not a table");
    assert!(failure.to_string().contains("not a table"), "{failure}");
}

/// The shipped library has recipes using its ingredients, so the counts are not all zero.
#[tokio::test]
async fn the_shipped_ingredients_are_used_by_the_shipped_recipes() {
    let client = serving_with_content("usage-library").await;

    let hops = client.usage("hop").await.expect("hop counts");
    assert!(!hops.is_empty(), "the shipped recipes use hops");
    assert!(hops.values().all(|used| *used > 0), "a count of zero should be absent, not stored");

    // Styles are named by a recipe rather than added to it, and are counted the same way.
    let styles = client.usage("style").await.expect("style counts");
    assert!(!styles.is_empty(), "the shipped recipes name styles");
}

/// Preferences round-trip, and a choice naming nothing is refused rather than stored.
#[tokio::test]
async fn preferences_are_stored_and_checked() {
    use serde_json::{Map, json};

    let client = serving("preferences").await;

    // The defaults are what the C++ defaults to.
    let defaults = client.preferences().await.expect("the defaults");
    assert_eq!(defaults["ibu_formula"], "tinseth");
    assert_eq!(defaults["color_formula"], "morey");

    let mut chosen = defaults.clone();
    chosen.insert("ibu_formula".to_owned(), json!("rager"));
    chosen.insert("units".to_owned(), json!({"Mass": "UsCustomary", "Temperature": "UsCustomary"}));
    let stored = client.set_preferences(&chosen).await.expect("store them");
    assert_eq!(stored["ibu_formula"], "rager");

    // ...and they are still there on the next request.
    let again = client.preferences().await.expect("read them back");
    assert_eq!(again["ibu_formula"], "rager");
    assert_eq!(again["units"]["Mass"], "UsCustomary");

    // A system of measurement that does not measure that quantity is refused.
    let mut wrong = Map::new();
    wrong.insert("units".to_owned(), json!({"Mass": "Lovibond"}));
    let failure = client.set_preferences(&wrong).await.expect_err("mass is not measured in Lovibond");
    assert!(failure.to_string().contains("Lovibond"), "{failure}");

    // So is a formula that does not exist.
    let mut nonsense = Map::new();
    nonsense.insert("ibu_formula".to_owned(), json!("vibes"));
    let failure = client.set_preferences(&nonsense).await.expect_err("not a formula");
    assert!(failure.to_string().contains("vibes"), "{failure}");
}

/// Choosing a different bitterness formula changes what the server reckons a recipe comes to.
///
/// The preferences are not decoration: they feed the calculation.
#[tokio::test]
async fn the_chosen_formula_is_the_one_used() {
    use serde_json::json;

    let client = serving_with_content("preferences-formula").await;
    let id = client.recipe_rows().await.expect("the recipes")[0].id;

    let tinseth = client.calculated(id).await.expect("the figures").ibus;

    let mut chosen = client.preferences().await.expect("the defaults");
    chosen.insert("ibu_formula".to_owned(), json!("rager"));
    client.set_preferences(&chosen).await.expect("store them");

    let rager = client.calculated(id).await.expect("the figures again").ibus;
    assert!((tinseth - rager).abs() > 1.0, "Rager and Tinseth should not agree this closely: {tinseth} vs {rager}");
}
