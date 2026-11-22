// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/tests/support/mod.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Standing up the API for a test.
//!
//! Requests go through the real router rather than calling handlers, so routing, extraction,
//! status codes and serialisation are all exercised.  No socket is involved: `tower` calls the
//! service directly, which is faster and gives the same answers.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use bt_api::{AppState, api};
use bt_schema::Namespace;
use bt_store::Store;
use serde_json::Value;
use tower::ServiceExt;

/// Show what the server logged, so a request that fails says why.
///
/// The API reports store failures in outline so that table and column names do not leak to a
/// caller; the detail goes to the log, which is where a test needs to look.
pub fn show_logs() {
    let _ = tracing_subscriber::fmt().with_test_writer().with_env_filter("warn,bt_api=debug,bt_store=debug").try_init();
}

/// The root of the repository, where the shipped data files are.
pub fn repository() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A fresh, empty database with the current schema, behind the real router.
///
/// Each test names its own so they do not tread on one another when run in parallel.
pub async fn app(label: &str) -> axum::Router {
    let path = std::env::temp_dir().join(format!("bt-api-{label}.sqlite"));
    let _ = std::fs::remove_file(&path);
    std::fs::File::create(&path).expect("create the database file");
    let store = Store::connect(&format!("sqlite://{}", path.display())).await.expect("connect");
    store.prepare(&Namespace::Default).await.expect("prepare");
    api().with_state(AppState::new(store, repository().join("data")))
}

/// A request body that is JSON.
pub fn json(value: Value) -> Option<(&'static str, String)> {
    Some(("application/json", value.to_string()))
}

/// A request body that is a file we are handing over as it stands.
pub fn text(body: &str) -> Option<(&'static str, String)> {
    Some(("text/plain", body.to_owned()))
}

/// Make a request and read back the status and the body.
///
/// A body that is not JSON comes back as `Value::Null`; the tests that send one check the status
/// and go and look at what was written instead.
pub async fn request(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<(&'static str, String)>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some((content_type, body)) => {
            builder.header("content-type", content_type).body(Body::from(body)).expect("build")
        }
        None => builder.body(Body::empty()).expect("build"),
    };
    let response = app.clone().oneshot(request).await.expect("call");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024 * 1024).await.expect("read");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}
