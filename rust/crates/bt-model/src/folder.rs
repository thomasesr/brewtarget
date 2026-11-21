// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-model/src/folder.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! Folders, which are rows that form a tree.
//!
//! Ported from `src/model/Folder.h`, `src/model/Folder.cpp` and `src/model/FolderPropertyBase.h`.
//!
//! A folder stores only its name and its parent, so a path like `foo/bar/hum` is reconstructed by
//! walking up the parents.  Each kind of thing keeps its own folder table -- there are eleven,
//! all the same shape -- which is what stops a Hop being filed in a Fermentable folder.
//!
//! Paths have no leading slash.  A folder's `full_path` ends with its own name and its `path` is
//! what comes before, including the trailing slash; both follow `Folder::doPath`.

use std::collections::{BTreeMap, BTreeSet};

/// One folder, independent of which of the eleven tables it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderNode {
    pub id: i64,
    pub name: String,
    /// `contained_in_folder_id`: the folder this one sits in, if any.
    pub parent_id: Option<i64>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FolderError {
    #[error("folder #{0} is contained in #{1}, which does not exist")]
    MissingParent(i64, i64),
    #[error("folders {0:?} contain each other")]
    Cycle(Vec<i64>),
}

/// The folders of one table, indexed so paths can be walked.
#[derive(Debug, Clone, Default)]
pub struct FolderTree {
    nodes: BTreeMap<i64, FolderNode>,
}

impl FolderTree {
    /// Build a tree, checking that it really is one.
    ///
    /// The C++ only guards against a folder being its own parent.  A longer cycle would send
    /// `doPath` round for ever, which matters more in a server than in a desktop application, so
    /// every chain is walked once up front and a cycle is an error rather than a hang.
    pub fn new(nodes: impl IntoIterator<Item = FolderNode>) -> Result<Self, FolderError> {
        let nodes: BTreeMap<i64, FolderNode> = nodes.into_iter().map(|node| (node.id, node)).collect();

        for node in nodes.values() {
            let mut seen = BTreeSet::from([node.id]);
            let mut current = node;
            while let Some(parent_id) = current.parent_id {
                let parent = nodes.get(&parent_id).ok_or(FolderError::MissingParent(current.id, parent_id))?;
                if !seen.insert(parent_id) {
                    return Err(FolderError::Cycle(seen.into_iter().collect()));
                }
                current = parent;
            }
        }
        Ok(Self { nodes })
    }

    pub fn get(&self, id: i64) -> Option<&FolderNode> {
        self.nodes.get(&id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &FolderNode> {
        self.nodes.values()
    }

    /// The folders containing this one, nearest first.
    pub fn ancestors(&self, id: i64) -> Vec<&FolderNode> {
        let mut ancestors = Vec::new();
        let mut current = self.nodes.get(&id);
        while let Some(node) = current {
            match node.parent_id.and_then(|parent_id| self.nodes.get(&parent_id)) {
                Some(parent) => {
                    ancestors.push(parent);
                    current = Some(parent);
                }
                None => break,
            }
        }
        ancestors
    }

    /// Everything before this folder's own name, with a trailing slash; empty at the root.
    ///
    /// Ported from `Folder::path`, which is `doPath("")`.
    pub fn path(&self, id: i64) -> Option<String> {
        self.do_path(id, String::new())
    }

    /// The whole path, ending in this folder's own name.
    ///
    /// Ported from `Folder::fullPath`, which is `doPath(name())`.
    pub fn full_path(&self, id: i64) -> Option<String> {
        let node = self.nodes.get(&id)?;
        self.do_path(id, node.name.clone())
    }

    fn do_path(&self, id: i64, start_with: String) -> Option<String> {
        let node = self.nodes.get(&id)?;
        let mut path = start_with;
        for ancestor in self.ancestors(node.id) {
            path = format!("{}/{}", ancestor.name, path);
        }
        Some(path)
    }

    /// The folder at a path, if there is one.
    ///
    /// Leading and trailing slashes are ignored, so `/foo/bar/` and `foo/bar` are the same place.
    pub fn find_by_path(&self, path: &str) -> Option<&FolderNode> {
        let wanted = normalise(path);
        self.nodes.values().find(|node| self.full_path(node.id).as_deref() == Some(wanted.as_str()))
    }

    /// The folders directly inside `parent_id`, or the root folders when that is `None`.
    pub fn children(&self, parent_id: Option<i64>) -> Vec<&FolderNode> {
        self.nodes.values().filter(|node| node.parent_id == parent_id).collect()
    }
}

/// Strip the leading and trailing slashes a path may arrive with.
///
/// Ported from the `normalised` lambda in `FolderCommon::subPath`.
pub fn normalise(path: &str) -> String {
    path.trim().trim_start_matches('/').trim_end_matches('/').to_owned()
}

/// Join two paths with a single slash between them.
///
/// Ported from `FolderCommon::joinPaths`: a trailing slash on the left and a leading slash on the
/// right are dropped, but nothing else is tidied.
pub fn join_paths(left_path: &str, right_path: &str) -> String {
    format!(
        "{}/{}",
        left_path.strip_suffix('/').unwrap_or(left_path),
        right_path.strip_prefix('/').unwrap_or(right_path)
    )
}

/// The part of `full_path` below `base_path`.
///
/// Ported from `FolderCommon::subPath`.  The comparison is by path component, not by string
/// prefix, so `foobar` is not treated as living inside `foo`.  Anything that is not actually
/// below the base comes back unchanged.
pub fn sub_path(base_path: &str, full_path: &str) -> String {
    let base = normalise(base_path);
    let full = normalise(full_path);
    if base.is_empty() {
        return full_path.to_owned();
    }
    if full == base {
        return String::new();
    }
    if !full.starts_with(&format!("{base}/")) {
        return full_path.to_owned();
    }
    full[base.len()..].to_owned()
}
