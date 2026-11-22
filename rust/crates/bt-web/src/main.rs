// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-web/src/main.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Brewtarget in a browser.
//!
//! All this does is start `bt_ui::App`.  Everything the application is lives in `bt-ui`, so the
//! desktop and mobile builds are the same few lines against a different Dioxus renderer.
//!
//! The API is taken to be on the same origin as the page, because `bt-server` serves both.

fn main() {
    dioxus::launch(bt_ui::App);
}
