// SPDX-License-Identifier: GPL-3.0-or-later
//
// bt-format/src/jsonc.rs is part of Brewtarget, and is copyright the following authors 2026:
//   • The Brewtarget contributors
//
// Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
// License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
// version.  See <http://www.gnu.org/licenses/>.

//! JSON that may contain comments.
//!
//! Strictly JSON has no comments, but `.beer` files are written by hand and use them heavily --
//! the example files in `testData/dotBeer/` are more comment than data in places -- and the C++
//! parses with Boost's `allow_comments` for exactly that reason.
//!
//! `serde_json` has no such option, so comments are blanked before parsing.  They are replaced
//! with spaces rather than removed so that every byte after them keeps its position, which means
//! a parse error still points at the right line and column of the file the user actually wrote.

/// Replace `//` and `/* */` comments with spaces, leaving everything else where it was.
///
/// Comment markers inside string literals are left alone: a hop called `Simcoe // 2024` is a
/// name, not a comment.
pub fn blank_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                // Copy the whole string literal, honouring backslash escapes.
                let start = index;
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index += 2,
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
                let end = index.min(bytes.len());
                out.push_str(&source[start..end]);
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    out.push(' ');
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let start = index;
                index += 2;
                while index < bytes.len() && !(bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/')) {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
                // Keep the newlines so later line numbers do not shift.
                for byte in &bytes[start..index] {
                    out.push(if *byte == b'\n' { '\n' } else { ' ' });
                }
            }
            _ => {
                // Advance a whole character, since a multi-byte one must not be split.
                let remainder = &source[index..];
                let character = remainder.chars().next().expect("index is on a character boundary");
                out.push(character);
                index += character.len_utf8();
            }
        }
    }
    out
}

/// Parse JSON that may contain comments.
pub fn from_str<T: serde::de::DeserializeOwned>(source: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(&blank_comments(source))
}

/// Parse JSON that may contain comments into an untyped value.
pub fn value_from_str(source: &str) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_str(&blank_comments(source))
}
