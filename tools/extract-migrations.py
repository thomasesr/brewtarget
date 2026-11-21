#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-migrations.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract the schema migrations from the legacy Qt/C++ tree.

``src/database/DatabaseSchemaHelper.cpp`` holds the upgrade path from schema version 1 to 23 as a
series of ``migrate_to_*`` functions, each building a list of SQL statements.  The SQL is
deliberately hard-coded there -- it must keep describing the schema as it was at that version, not
as it is now -- so it is exactly the kind of thing to lift wholesale rather than retype.

Statements are emitted with the dialect-specific parts left as placeholders (``{{type:double}}``,
``{{primary_key}}``, ``{{add_foreign_key}}``, ``{{cascade}}``), which ``bt-schema`` fills in for the
database it is talking to.

Migrations whose statements are computed rather than declared cannot be lifted this way; they are
reported as unsupported and hand-ported in Rust instead.  Nothing is dropped silently.

    python3 tools/extract-migrations.py --out schema/migrations.json

See PLAN.md Phase 2.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

# Python puts this script's directory on sys.path, so this resolves to tools/cpputil.py.
from cpputil import match_balanced, split_top_level, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
SRC = REPO / "src" / "database" / "DatabaseSchemaHelper.cpp"

# `db.getDbNativeTypeName<T>()` and friends vary by database.  Keep them symbolic here and let the
# Rust renderer substitute, so that one extraction serves all three dialects.
NATIVE_TYPE_MARKERS = {
    "QString": "{{type:string}}",
    "double": "{{type:double}}",
    "int": "{{type:int}}",
    "unsigned int": "{{type:uint}}",
    "bool": "{{type:bool}}",
    "QDate": "{{type:date}}",
}
CALL_MARKERS = {
    "getDbNativePrimaryKeyDeclaration": "{{primary_key}}",
    "getSqlToAddColumnAsForeignKey": "{{add_foreign_key}}",
    "getCascade": "{{cascade}}",
}


class Unsupported(Exception):
    """Raised when an expression is not of a form this script can lift."""


# ── Expression evaluation ─────────────────────────────────────────────────────────────────────

STRING_LITERAL_RE = re.compile(r'"((?:[^"\\]|\\.)*)"')


def string_literals(text: str) -> str | None:
    """Join adjacent C++ string literals, as the compiler would.  None if `text` is not literals."""
    pieces = STRING_LITERAL_RE.findall(text)
    if not pieces or STRING_LITERAL_RE.sub("", text).strip():
        return None
    return "".join(pieces).encode().decode("unicode_escape")


def qstring_arg(template: str, values: list[str]) -> str:
    """Apply Qt's `QString::arg` semantics.

    A single-argument call replaces the lowest-numbered remaining place marker; the multi-argument
    form assigns its arguments to the place markers present, in ascending order, in one pass.  The
    difference matters when the markers are not a contiguous run from %1.
    """
    markers = sorted({int(n) for n in re.findall(r"%(\d+)", template)})
    if len(values) > len(markers):
        raise Unsupported(f"{len(values)} arguments for place markers {markers}")
    result = template
    for marker, value in zip(markers, values):
        result = result.replace(f"%{marker}", value)
    return result


def split_arguments(text: str) -> list[str]:
    """Split a call's argument list on top-level commas."""
    args, depth, current = [], 0, ""
    for char in text:
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        if char == "," and depth == 0:
            args.append(current)
            current = ""
        else:
            current += char
    if current.strip():
        args.append(current)
    return [a.strip() for a in args]


def evaluate(expr: str, bindings: dict[str, str]) -> str:
    """Evaluate a C++ expression that yields a SQL string."""
    expr = expr.strip()

    native = re.fullmatch(r"db\.getDbNativeTypeName<\s*([\w ]+?)\s*>\(\)", expr)
    if native:
        key = native.group(1)
        if key not in NATIVE_TYPE_MARKERS:
            raise Unsupported(f"unknown native type {key!r}")
        return NATIVE_TYPE_MARKERS[key]

    call = re.fullmatch(r"db\.(\w+)\(\)", expr)
    if call:
        if call.group(1) not in CALL_MARKERS:
            raise Unsupported(f"unknown Database call {call.group(1)}")
        return CALL_MARKERS[call.group(1)]

    if expr in bindings:
        return bindings[expr]

    if expr.startswith("QString("):
        open_paren, close_paren = match_balanced(expr, len("QString") , "(", ")")
        result = evaluate(expr[open_paren + 1 : close_paren - 1], bindings)
        rest = expr[close_paren:].strip()
        while rest:
            if not rest.startswith(".arg("):
                raise Unsupported(f"trailing {rest!r}")
            args_open, args_close = match_balanced(rest, len(".arg") - 1, "(", ")")
            values = [evaluate(a, bindings) for a in split_arguments(rest[args_open + 1 : args_close - 1])]
            result = qstring_arg(result, values)
            rest = rest[args_close:].strip()
        return result

    literal = string_literals(expr)
    if literal is not None:
        return literal

    raise Unsupported(f"cannot evaluate {expr!r}")


def evaluate_bind(expr: str, bindings: dict[str, str]):
    """Evaluate a `QVariant(...)` bind value to JSON."""
    expr = expr.strip()
    inner = re.fullmatch(r"QVariant\((.*)\)", expr, re.S)
    if not inner:
        raise Unsupported(f"bind value {expr!r}")
    value = inner.group(1).strip()
    if value in ("true", "false"):
        return value == "true"
    if re.fullmatch(r"-?\d+", value):
        return int(value)
    if re.fullmatch(r"-?\d+\.\d*", value):
        return float(value)
    if value == "DatabaseSchemaHelper::latestVersion":
        return {"$ref": "latest_version"}
    return evaluate(value, bindings)


def parse_entry(entry: str, bindings: dict[str, str]) -> dict:
    """Parse one `{sql, binds, onlyRunIfPriorQueryHadResults}` initialiser."""
    parts = split_arguments(entry[1:-1])
    if not parts:
        raise Unsupported("empty query initialiser")
    statement = {"sql": evaluate(parts[0], bindings)}
    if len(parts) > 1 and parts[1].strip() not in ("{}", ""):
        binds = split_arguments(parts[1].strip()[1:-1])
        statement["binds"] = [evaluate_bind(b, bindings) for b in binds]
    if len(parts) > 2:
        flag = parts[2].strip()
        if flag not in ("true", "false"):
            raise Unsupported(f"unexpected third field {flag!r}")
        statement["only_if_prior_had_results"] = flag == "true"
    return statement


# ── Function body walking ─────────────────────────────────────────────────────────────────────

VECTOR_DECL_RE = re.compile(r"QVector<QueryAndParameters>\s*(?:const\s+)?(\w+)\s*(?=\{)")
FOREACH_RE = re.compile(r"\s*(?:char const \*|auto)\s*(?:const\s+)?(\w+)\s*:\s*\{(.*)\}\s*$", re.S)


def collect(text: str, bindings: dict[str, str], vector: str) -> list[dict]:
    """Walk statements, gathering the queries they append."""
    statements, i = [], 0
    while i < len(text):
        remainder = text[i:]
        i += len(remainder) - len(remainder.lstrip())
        if i >= len(text):
            break

        if text.startswith(f"{vector}.append(", i):
            start, end = match_balanced(text, i, "(", ")")
            statements.append(parse_entry(text[start + 1 : end - 1].strip(), bindings))
            i = text.index(";", end) + 1
        elif re.match(r"for\s*\(", text[i:]):
            header_start, header_end = match_balanced(text, i, "(", ")")
            header = FOREACH_RE.fullmatch(text[header_start + 1 : header_end - 1])
            if not header:
                raise Unsupported(f"loop {text[header_start:header_end]!r}")
            variable = header.group(1)
            items = [string_literals(item) for item in split_arguments(header.group(2))]
            if any(item is None for item in items):
                raise Unsupported("loop over non-literals")
            body_start, body_end = match_balanced(text, header_end)
            for item in items:
                statements += collect(text[body_start + 1 : body_end - 1], {**bindings, variable: item}, vector)
            i = body_end
        elif text.startswith("return", i):
            break
        else:
            raise Unsupported(f"statement {text[i : i + 70]!r}")
    return statements


def extract_function(body: str) -> list[dict]:
    """Extract the statements one `migrate_to_*` function runs."""
    declaration = VECTOR_DECL_RE.search(body)
    if not declaration:
        if re.search(r"return\s+true\s*;", body) and "executeSqlQueries" not in body:
            return []  # a deliberate no-op migration
        raise Unsupported("no QVector<QueryAndParameters> declaration")
    vector = declaration.group(1)
    start, end = match_balanced(body, declaration.end())
    statements = [parse_entry(entry, {}) for entry in split_top_level(body[start + 1 : end - 1])]

    tail = body[body.index(";", end) + 1 :]
    statements += collect(tail, {}, vector)
    return statements
