// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-store/src/default_content.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Seeding a database with the ingredients Brewtarget ships.
//!
//! Ported from `src/database/DefaultContentLoader.cpp`.
//!
//! The content is a numbered sequence of files -- `DefaultContent001-…` through
//! `DefaultContent006-…` -- in three different formats, and the database records how far through
//! that sequence it has got.  A new release adds a file and raises the number; an existing
//! database then loads only what it has not seen, which is why this is a version rather than a
//! flag.
//!
//! The C++ asks the user before loading.  That is a question for whatever is driving this, not
//! for the storing layer, so it is simply done when called.

use std::path::{Path, PathBuf};

use bt_format::document::{Document, read_beerjson, read_beerxml, read_dotbeer};
use bt_format::{jsonc, xml};

use crate::{Store, StoreError, import};

/// `DefaultContentLoader::availableContentVersion`.
pub const AVAILABLE_CONTENT_VERSION: i32 = 6;

/// Where the recipes that come with the application are filed.
///
/// `FOLDER_PATH_FOR_SUPPLIED_RECIPES` in the C++, which is the application's own name.
pub const SUPPLIED_RECIPES_FOLDER: &str = "brewtarget";

/// What loading the default content did.
#[derive(Debug, Clone, PartialEq)]
pub enum Loaded {
    /// The database already had everything this release ships.
    NothingToDo { version: i32 },
    /// Content was loaded, up to the version named.
    UpTo { version: i32, files: Vec<String>, rows_written: usize },
}

/// How far through the sequence this database has got.
pub async fn loaded_version(store: &Store) -> Result<i32, StoreError> {
    let sql = "SELECT default_content_version FROM settings WHERE id = 1";
    let found: Option<Option<i32>> =
        crate::dispatch!(store, |pool| { sqlx::query_scalar(sql).fetch_optional(pool).await? });
    Ok(found.flatten().unwrap_or(0))
}

/// Record how far through the sequence this database has got.
pub async fn set_loaded_version(store: &Store, version: i32) -> Result<(), StoreError> {
    let sql = store.placeholders("UPDATE settings SET default_content_version = ? WHERE id = 1");
    crate::dispatch!(store, |pool| {
        sqlx::query(&sql).bind(version).execute(pool).await?;
    });
    Ok(())
}

/// The content file for one step of the sequence, if there is one.
///
/// The files are named `DefaultContentNNN-something.extension`, and exactly one must match each
/// number: two would make the order ambiguous.
pub fn file_for(directory: &Path, version: i32) -> Result<Option<PathBuf>, StoreError> {
    let prefix = format!("DefaultContent{version:03}-");
    let mut matching: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|source| StoreError::ContentDirectory { path: directory.display().to_string(), source })?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.file_name()?.to_str()?.starts_with(&prefix).then_some(path)
        })
        .collect();
    matching.sort();

    match matching.len() {
        0 => Ok(None),
        1 => Ok(Some(matching.remove(0))),
        _ => Err(StoreError::AmbiguousContentFile { prefix, found: matching.len() }),
    }
}

/// Read one content file, choosing the format from its extension.
pub fn read_file(path: &Path) -> Result<Document, StoreError> {
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default();
    let bytes = std::fs::read(path)
        .map_err(|source| StoreError::ContentDirectory { path: path.display().to_string(), source })?;

    let document = match extension {
        // BeerXML declares ISO-8859-1, where every byte is one character.
        "xml" => {
            let text: String = bytes.iter().map(|byte| *byte as char).collect();
            let parsed = xml::read(&text).map_err(|error| StoreError::ContentFormat {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;
            read_beerxml(&parsed)
        }
        "json" | "beer" => {
            let text = String::from_utf8_lossy(&bytes);
            let parsed = jsonc::value_from_str(&text).map_err(|error| StoreError::ContentFormat {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;
            if extension == "beer" { read_dotbeer(&parsed) } else { read_beerjson(&parsed) }
        }
        other => {
            return Err(StoreError::ContentFormat {
                path: path.display().to_string(),
                message: format!("{other} is not a format this reads"),
            });
        }
    };
    document.map_err(|error| StoreError::ContentFormat { path: path.display().to_string(), message: error.to_string() })
}

/// Load whatever this database has not yet seen.
///
/// Files are loaded in order, because the sequence is how the content is versioned: a release
/// that adds `DefaultContent007-` expects everything before it to be there already.
pub async fn load(store: &Store, directory: &Path) -> Result<Loaded, StoreError> {
    let already = loaded_version(store).await?;
    if already >= AVAILABLE_CONTENT_VERSION {
        return Ok(Loaded::NothingToDo { version: already });
    }

    let mut files = Vec::new();
    let mut rows_written = 0;
    for version in (already + 1)..=AVAILABLE_CONTENT_VERSION {
        let Some(path) = file_for(directory, version)? else {
            continue;
        };
        let document = read_file(&path)?;
        let imported = import::import(store, &document).await?;
        rows_written += imported.rows_written;
        files.push(path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
    }

    set_loaded_version(store, AVAILABLE_CONTENT_VERSION).await?;
    Ok(Loaded::UpTo { version: AVAILABLE_CONTENT_VERSION, files, rows_written })
}
