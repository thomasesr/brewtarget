#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/cpputil.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Minimal C++ source handling shared by the extractors in this directory.

Not a parser -- just enough brace and comment awareness to pull declarative data out of
``src/database/``.  See PLAN.md Phase 1 and Phase 2.
"""

from __future__ import annotations


def strip_comments(text: str) -> str:
    """Remove // and /* */ comments without touching string literals."""
    out, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            j = i + 1
            while j < n and not (text[j] == '"' and text[j - 1] != "\\"):
                j += 1
            out.append(text[i : j + 1])
            i = j + 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


def match_balanced(text: str, start: int, opener: str = "{", closer: str = "}") -> tuple[int, int]:
    """Return (start, end) of the balanced group whose opener is at or after `start`."""
    i = text.index(opener, start)
    depth, j = 0, i
    while j < len(text):
        if text[j] == opener:
            depth += 1
        elif text[j] == closer:
            depth -= 1
            if depth == 0:
                return i, j + 1
        j += 1
    raise ValueError(f"unbalanced {opener!r} from offset {start}")


def split_top_level(body: str) -> list[str]:
    """Split a brace body into its top-level `{...}` groups."""
    groups, i = [], 0
    while True:
        try:
            s, e = match_balanced(body, i)
        except ValueError:
            return groups
        groups.append(body[s:e])
        i = e
