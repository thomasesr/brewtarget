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
