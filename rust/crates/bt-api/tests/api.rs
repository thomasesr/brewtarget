// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/tests/api.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Driving the API over HTTP.
//!
//! The groundwork is in `support`; these are the shapes every resource route has in common.

mod support;

use axum::http::StatusCode;
use serde_json::json;
use support::{app, json as body, request};

/// A load balancer has to be able to ask whether the process is alive, and gets told only that.
#[tokio::test]
async fn health_says_only_that_it_is_alive() {
    let app = app("health").await;
    let (status, health) = request(&app, "GET", "/healthz", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(health, json!({"status": "ok"}));
    // No version, no schema, nothing about what is inside.
    assert_eq!(health.as_object().expect("an object").len(), 1);
}

#[tokio::test]
async fn a_hop_can_be_created_read_updated_and_deleted() {
    let app = app("crud").await;

    let (status, created) =
        request(&app, "POST", "/api/hop", body(json!({"name": "Saaz", "alpha_pct": 3.5, "deleted": false}))).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["id"].as_i64().expect("the database assigned a key");
    assert!(id > 0);
    assert_eq!(created["name"], "Saaz");

    let (status, read) = request(&app, "GET", &format!("/api/hop/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(read["alpha_pct"], 3.5);

    let (status, updated) =
        request(&app, "PUT", &format!("/api/hop/{id}"), body(json!({"name": "Saaz", "alpha_pct": 4.0}))).await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["alpha_pct"], 4.0);
    assert_eq!(updated["id"], id, "the key in the path is the one that counts");

    let (status, _) = request(&app, "DELETE", &format!("/api/hop/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Deleting is soft: the row stays so that recipes using it still resolve.
    let (status, after) = request(&app, "GET", &format!("/api/hop/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after["deleted"], true);
}

#[tokio::test]
async fn asking_for_something_that_is_not_there_says_so() {
    let app = app("missing").await;
    let (status, failure) = request(&app, "GET", "/api/hop/9999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(failure["kind"], "not_found");
    assert!(failure["message"].as_str().expect("a message").contains("9999"));

    let (status, _) = request(&app, "PUT", "/api/hop/9999", body(json!({"name": "ghost"}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The API describes itself, so a client can be generated from it rather than hand-written.
#[tokio::test]
async fn the_api_describes_itself() {
    let app = app("openapi").await;
    let (status, document) = request(&app, "GET", "/api/openapi.json", None).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(document["info"]["title"], "Brewtarget");
    let paths = document["paths"].as_object().expect("the paths");
    // Every table has a resource group, and the hand-written routes are in there too.
    assert!(paths.contains_key("/api/hop"), "no hops");
    assert!(paths.contains_key("/api/hop/{id}"));
    assert!(paths.contains_key("/api/recipes/{id}/calculated"));
    assert!(paths.contains_key("/api/inventory/{kind}/{id}"));
    assert!(paths.contains_key("/api/import"));
    assert_eq!(paths.len(), 39 * 2 + 3 + 3 + 2, "one group per table, plus the hand-written routes");

    // The schemas the routes mention are described, not left as bare objects.
    let schemas = document["components"]["schemas"].as_object().expect("the schemas");
    for named in ["HopRow", "RecipeRow", "Calculated", "Holding", "ErrorBody"] {
        assert!(schemas.contains_key(named), "{named} is not described");
    }
    // A nullable column is described as nullable: almost every column in this schema is.
    assert_eq!(schemas["HopRow"]["properties"]["alpha_pct"]["type"], serde_json::json!(["number", "null"]));
    assert_eq!(schemas["HopRow"]["properties"]["id"]["type"], "integer");
}
