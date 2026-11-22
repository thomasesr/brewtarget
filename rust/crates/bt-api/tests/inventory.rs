// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/tests/inventory.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Buying and using an ingredient, and what is left afterwards.

mod support;

use axum::http::StatusCode;
use serde_json::json;
use support::{app, json as body, request};

/// Buy some hops, use some, and see what is left.
#[tokio::test]
async fn what_is_left_is_what_was_bought_less_what_was_used() {
    let app = app("inventory").await;

    let (_, hop) = request(&app, "POST", "/api/hop", body(json!({"name": "Fuggle", "alpha_pct": 4.5}))).await;
    let hop_id = hop["id"].as_i64().expect("an id");

    // Two lots, bought in different units, which is the point of holding a unit per lot: 500 g
    // and 0.25 kg are three quarters of a kilogram between them.
    for (quantity, unit) in [(500.0, "grams"), (0.25, "kilograms")] {
        let (status, lot) = request(
            &app,
            "POST",
            "/api/hop_stock_purchase",
            body(json!({"name": "Fuggle", "hop_id": hop_id, "quantity": quantity, "unit": unit})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{lot}");
    }

    let (status, holding) = request(&app, "GET", &format!("/api/inventory/hop/{hop_id}"), None).await;
    assert_eq!(status, StatusCode::OK, "{holding}");
    assert_eq!(holding["unit"], "kilograms", "totals are in the canonical unit");
    assert!((holding["quantity"].as_f64().expect("a quantity") - 0.75).abs() < 1e-9, "{holding}");
    assert_eq!(holding["on_hand"], true);
    assert_eq!(holding["lots"].as_array().expect("the lots").len(), 2);

    // Brew with 200 g of the first lot.
    let first = holding["lots"][0]["purchase_id"].as_i64().expect("a lot");
    let (status, used) = request(
        &app,
        "POST",
        "/api/hop_stock_use",
        body(json!({"name": "brewing", "hop_purchase_id": first, "quantity_used": 0.2, "reason": "used"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{used}");

    let (_, holding) = request(&app, "GET", &format!("/api/inventory/hop/{hop_id}"), None).await;
    assert!((holding["quantity"].as_f64().expect("a quantity") - 0.55).abs() < 1e-9, "{holding}");
    assert!((holding["lots"][0]["remaining"].as_f64().expect("what is left") - 0.3).abs() < 1e-9, "{holding}");
}

/// An ingredient nobody has bought has no holding, and asking for one says so rather than
/// returning a zero that reads as "we have none left" when it means "we never had any".
#[tokio::test]
async fn an_ingredient_never_bought_has_no_holding() {
    let app = app("inventory-empty").await;
    let (_, hop) = request(&app, "POST", "/api/hop", body(json!({"name": "Saaz"}))).await;
    let hop_id = hop["id"].as_i64().expect("an id");

    let (status, failure) = request(&app, "GET", &format!("/api/inventory/hop/{hop_id}"), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(failure["kind"], "not_found");

    let (status, holdings) = request(&app, "GET", "/api/inventory", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(holdings.as_array().expect("a list").len(), 0);
}

/// A lot that has been used up stays listed, but stops counting towards the total.
#[tokio::test]
async fn a_spent_lot_is_still_listed() {
    let app = app("inventory-spent").await;
    let (_, misc) = request(&app, "POST", "/api/misc", body(json!({"name": "Irish Moss"}))).await;
    let misc_id = misc["id"].as_i64().expect("an id");
    let (_, lot) = request(
        &app,
        "POST",
        "/api/misc_stock_purchase",
        body(json!({"name": "Irish Moss", "misc_id": misc_id, "quantity": 0.1, "unit": "kilograms"})),
    )
    .await;
    let lot_id = lot["id"].as_i64().expect("an id");
    request(
        &app,
        "POST",
        "/api/misc_stock_use",
        body(json!({"name": "all of it", "misc_purchase_id": lot_id, "quantity_used": 0.1})),
    )
    .await;

    let (_, holding) = request(&app, "GET", &format!("/api/inventory/misc/{misc_id}"), None).await;
    assert_eq!(holding["quantity"], 0.0);
    assert_eq!(holding["on_hand"], false);
    assert_eq!(holding["lots"].as_array().expect("the lots").len(), 1, "an empty jar is still a jar");

    // ...and asking only for what is on hand leaves it out.
    let (_, on_hand) = request(&app, "GET", "/api/inventory?on_hand=true", None).await;
    assert_eq!(on_hand.as_array().expect("a list").len(), 0);
}

/// The path says which kind of thing is being counted, and an unknown one is a bad request
/// rather than an empty list that looks like an answer.
#[tokio::test]
async fn something_that_cannot_be_stocked_is_refused() {
    let app = app("inventory-kind").await;
    let (status, failure) = request(&app, "GET", "/api/inventory/recipe", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(failure["message"].as_str().expect("a message").contains("fermentable"), "{failure}");
}
