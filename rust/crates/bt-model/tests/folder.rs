// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-model/tests/folder.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Checks on the folder tree.
//!
//! The paths these produce are what `migrate_to_20` took apart and what the UI puts back
//! together, so they have to match `Folder::doPath` exactly -- including `path()` keeping its
//! trailing slash while `full_path()` does not have a leading one.

use bt_model::folder::{FolderError, FolderNode, FolderTree, join_paths, normalise, sub_path};

fn node(id: i32, name: &str, parent_id: Option<i32>) -> FolderNode {
    FolderNode { id, name: name.to_owned(), parent_id, deleted: false }
}

/// foo (1) → bar (2) → hum (3), with a second root `other` (4).
fn tree() -> FolderTree {
    FolderTree::new([node(1, "foo", None), node(2, "bar", Some(1)), node(3, "hum", Some(2)), node(4, "other", None)])
        .expect("a valid tree")
}

#[test]
fn a_full_path_is_built_from_the_parent_chain() {
    let tree = tree();
    assert_eq!(tree.full_path(1).as_deref(), Some("foo"));
    assert_eq!(tree.full_path(2).as_deref(), Some("foo/bar"));
    assert_eq!(tree.full_path(3).as_deref(), Some("foo/bar/hum"));
    assert_eq!(tree.full_path(99), None);
}

/// `path()` is everything above the folder itself, and keeps the trailing slash the C++ leaves
/// on it because it is `doPath("")`.
#[test]
fn path_is_everything_above_the_folder() {
    let tree = tree();
    assert_eq!(tree.path(1).as_deref(), Some(""));
    assert_eq!(tree.path(2).as_deref(), Some("foo/"));
    assert_eq!(tree.path(3).as_deref(), Some("foo/bar/"));
}

#[test]
fn ancestors_run_from_nearest_to_root() {
    let tree = tree();
    let names: Vec<&str> = tree.ancestors(3).into_iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, ["bar", "foo"]);
    assert!(tree.ancestors(1).is_empty());
}

#[test]
fn folders_can_be_found_by_path_however_it_is_spelled() {
    let tree = tree();
    assert_eq!(tree.find_by_path("foo/bar/hum").map(|node| node.id), Some(3));
    assert_eq!(tree.find_by_path("/foo/bar/hum/").map(|node| node.id), Some(3));
    assert_eq!(tree.find_by_path("  foo/bar  ").map(|node| node.id), Some(2));
    assert_eq!(tree.find_by_path("foo/nope"), None);
}

#[test]
fn children_are_listed_per_parent_with_roots_under_none() {
    let tree = tree();
    let roots: Vec<&str> = tree.children(None).into_iter().map(|node| node.name.as_str()).collect();
    assert_eq!(roots, ["foo", "other"]);
    let under_foo: Vec<&str> = tree.children(Some(1)).into_iter().map(|node| node.name.as_str()).collect();
    assert_eq!(under_foo, ["bar"]);
    assert!(tree.children(Some(3)).is_empty());
}

/// The C++ only guards against a folder being its own parent, which would send `doPath` round for
/// ever.  A longer cycle does the same thing, and hanging is worse in a server than on a desktop,
/// so every chain is checked when the tree is built.
#[test]
fn a_cycle_is_an_error_rather_than_a_hang() {
    assert!(matches!(FolderTree::new([node(1, "self", Some(1))]), Err(FolderError::Cycle(_))));

    let longer = FolderTree::new([node(1, "a", Some(3)), node(2, "b", Some(1)), node(3, "c", Some(2))]);
    assert!(matches!(longer, Err(FolderError::Cycle(_))), "{longer:?}");
}

#[test]
fn a_folder_whose_parent_is_missing_is_an_error() {
    let orphan = FolderTree::new([node(1, "orphan", Some(42))]);
    assert_eq!(orphan.expect_err("missing parent"), FolderError::MissingParent(1, 42));
}

#[test]
fn joining_paths_leaves_exactly_one_slash() {
    assert_eq!(join_paths("foo", "bar"), "foo/bar");
    assert_eq!(join_paths("foo/", "bar"), "foo/bar");
    assert_eq!(join_paths("foo", "/bar"), "foo/bar");
    assert_eq!(join_paths("foo/", "/bar"), "foo/bar");
    assert_eq!(join_paths("", ""), "/");
}

/// `sub_path` compares by path component.  `foobar` is not inside `foo`, however much it looks
/// like it is as a string prefix.
#[test]
fn sub_path_compares_components_not_string_prefixes() {
    assert_eq!(sub_path("foo", "foo/bar/hum"), "/bar/hum");
    assert_eq!(sub_path("/foo/", "foo/bar"), "/bar");
    assert_eq!(sub_path("foo", "foo"), "");
    assert_eq!(sub_path("foo", "foobar/hum"), "foobar/hum", "foobar does not live inside foo");
    assert_eq!(sub_path("", "foo/bar"), "foo/bar");
    assert_eq!(sub_path("other", "foo/bar"), "foo/bar");
}

#[test]
fn normalising_strips_the_slashes_a_path_may_arrive_with() {
    assert_eq!(normalise("/foo/bar/"), "foo/bar");
    assert_eq!(normalise("  /foo/  "), "foo");
    assert_eq!(normalise("/"), "");
    assert_eq!(normalise(""), "");
}
