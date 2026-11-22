// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-api/src/state.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! What a request needs to do its work.
//!
//! A handler is given its store rather than reaching for one.  There is no global pool and no
//! ambient current database, which is forward-compatibility constraint 2 in PLAN.md §3.3: in
//! Stage 2 every request gets the store belonging to whoever made it, and the only thing that
//! changes is where [`AppState::store`] gets it from.

use std::sync::Arc;

use bt_calc::ColorFormula;
use bt_calc::recipe_ibu::IbuSettings;
use bt_store::Store;

/// Settings that are a user's preference in the desktop application.
///
/// They live here rather than in `bt-calc` because a calculation crate has no business holding a
/// preference.  In Stage 2 these come from the account making the request.
#[derive(Debug, Clone, Copy, Default)]
pub struct Preferences {
    pub color_formula: ColorFormula,
    pub ibu: IbuSettings,
}

/// Everything the routes share.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    /// Where the files that ship with the application live.  Read, never written.
    data_directory: std::path::PathBuf,
    /// Where what the brewer has chosen is kept.  Written, so it must not be the content
    /// directory -- that is part of the installation, and in a container it is read-only.
    state_directory: std::path::PathBuf,
}

impl AppState {
    /// Everything the routes share, with the shipped content and the state in one place.
    ///
    /// Convenient for a test; `with_state_directory` is what a server wants, since the two are
    /// not the same thing.
    pub fn new(store: Store, data_directory: impl Into<std::path::PathBuf>) -> Self {
        let data_directory = data_directory.into();
        Self::with_state_directory(store, data_directory.clone(), data_directory)
    }

    /// As [`AppState::new`], but keeping what the brewer has chosen somewhere writable.
    pub fn with_state_directory(
        store: Store,
        data_directory: impl Into<std::path::PathBuf>,
        state_directory: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                store,
                data_directory: data_directory.into(),
                state_directory: state_directory.into(),
            }),
        }
    }

    /// The store this request should act on.
    ///
    /// Stage 2 replaces the body of this with a lookup keyed by the authenticated account; every
    /// handler already asks for its store this way, so none of them change.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// What the brewer has chosen, read fresh.
    ///
    /// Read each time rather than cached, so that changing a preference takes effect on the
    /// next request without the server being restarted.  It is one small file.
    pub fn preferences(&self) -> Preferences {
        let chosen = crate::preferences::Preferences::load(self.state_directory());
        Preferences { color_formula: chosen.color(), ibu: chosen.ibu() }
    }

    /// Where the files that ship with the application are.
    pub fn data_directory(&self) -> &std::path::Path {
        &self.inner.data_directory
    }

    /// Where what the brewer has chosen is kept.
    pub fn state_directory(&self) -> &std::path::Path {
        &self.inner.state_directory
    }
}
