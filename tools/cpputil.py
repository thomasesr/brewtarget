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


def scan(text: str):
    """Yield `(index, character)` for every character outside a string or character literal.

    The SQL in `src/database/` is full of parentheses and braces inside string literals, so any
    depth counting that does not skip literals will go badly wrong.
    """
    i, n = 0, len(text)
    while i < n:
        char = text[i]
        if char in "\"'":
            quote, i = char, i + 1
            while i < n and not (text[i] == quote and text[i - 1] != "\\"):
                i += 1
            i += 1
            continue
        yield i, char
        i += 1


def match_balanced(text: str, start: int, opener: str = "{", closer: str = "}") -> tuple[int, int]:
    """Return (start, end) of the balanced group whose opener is at or after `start`."""
    begin, depth = None, 0
    for index, char in scan(text[start:]):
        if char == opener:
            if begin is None:
                begin = start + index
            depth += 1
        elif char == closer and begin is not None:
            depth -= 1
            if depth == 0:
                return begin, start + index + 1
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


def strip_preprocessor_conditionals(text: str) -> str:
    """Keep the first branch of every `#if`/`#elif`/`#else`/`#endif` group, blanking the rest.

    `DatabaseSchemaHelper.cpp` guards a Qt 6.4 API behind `#if`, with the two branches opening a
    loop body that only one of them closes.  Taken literally the braces do not balance, so pick a
    branch before trying to walk them.  Lines are blanked rather than removed so that line numbers
    still line up with the original file.
    """
    kept, stack = [], []
    for line in text.split("\n"):
        directive = line.lstrip()
        if directive.startswith(("#if", "#ifdef", "#ifndef")):
            stack.append(True)
            kept.append("")
        elif directive.startswith(("#else", "#elif")):
            if stack:
                stack[-1] = False
            kept.append("")
        elif directive.startswith("#endif"):
            if stack:
                stack.pop()
            kept.append("")
        else:
            kept.append(line if all(stack) else "")
    return "\n".join(kept)
