// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/openapi.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! A description of the API, for whatever wants to generate a client from it.
//!
//! Four pieces: the generated resource groups, and the three hand-written sets of routes.  Each
//! names its own paths and the types they mention, and they are folded together here so that
//! adding a route means annotating it rather than remembering to list it twice.

use utoipa::OpenApi;
use utoipa::openapi::OpenApi as Document;

/// The parts that are not routes: what the API is and where it is.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Brewtarget",
        description = "The brewing database, over HTTP.  This build has no authentication: \
                       anything that can reach it has full control of the database.",
        license(name = "GPL-3.0-or-later", url = "https://www.gnu.org/licenses/gpl-3.0.html"),
    ),
    tags(
        (name = "recipes", description = "Recipes, and what they work out to"),
        (name = "inventory", description = "What is on hand"),
        (name = "transfer", description = "Whole libraries in and out"),
    ),
)]
struct About;

/// The whole API, described.
pub fn document() -> Document {
    let mut document = About::openapi();
    document.merge(crate::generated::routes::TableApi::openapi());
    document.merge(crate::recipes::RecipeApi::openapi());
    document.merge(crate::inventory::InventoryApi::openapi());
    document.merge(crate::transfer::TransferApi::openapi());
    document
}
