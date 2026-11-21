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
from cpputil import match_balanced, scan, split_top_level, strip_comments, strip_preprocessor_conditionals

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
# `getSqlToAddColumnAsForeignKey` returns a template with its own %1..%4, so its arguments travel
# with the marker rather than being substituted here.
TEMPLATE_MARKERS = {"{{add_foreign_key}}"}

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
    args, depth, cuts = [], 0, []
    for index, char in scan(text):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "," and depth == 0:
            cuts.append(index)
    previous = 0
    for cut in cuts:
        args.append(text[previous:cut])
        previous = cut + 1
    args.append(text[previous:])
    return [a.strip() for a in args if a.strip()]


def evaluate(expr: str, bindings: dict[str, str]) -> str:
    """Evaluate a C++ expression that yields a SQL string."""
    expr = expr.strip()

    native = re.fullmatch(r"db\.getDbNativeTypeName<\s*([\w ]+?)\s*>\s*\(\)", expr)
    if native:
        key = native.group(1)
        if key not in NATIVE_TYPE_MARKERS:
            raise Unsupported(f"unknown native type {key!r}")
        return NATIVE_TYPE_MARKERS[key]

    call = re.fullmatch(r"db\.(\w+)\s*\(\)", expr)
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
            if result in TEMPLATE_MARKERS:
                result = f"{result[:-2]}:{'|'.join(values)}}}}}"
            else:
                result = qstring_arg(result, values)
            rest = rest[args_close:].strip()
        return result

    ternary = split_ternary(expr)
    if ternary:
        condition, when_true, when_false = ternary
        return evaluate(when_true if test(condition, bindings) else when_false, bindings)

    unwrapped = re.fullmatch(r"QString\{(.*)\}", expr, re.S)
    if unwrapped:
        return evaluate(unwrapped.group(1), bindings)

    literal = string_literals(expr)
    if literal is not None:
        return literal

    raise Unsupported(f"cannot evaluate {expr!r}")


def split_ternary(expr: str) -> tuple[str, str, str] | None:
    """Split `cond ? a : b` on its top-level operators, ignoring any inside string literals."""
    depth, question, colon = 0, None, None
    for index, char in scan(expr):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif depth == 0 and char == "?" and question is None:
            question = index
        elif depth == 0 and char == ":" and question is not None and not expr.startswith("::", index):
            colon = index
            break
    if question is None or colon is None:
        return None
    return expr[:question], expr[question + 1 : colon], expr[colon + 1 :]


def test(condition: str, bindings: dict[str, str]) -> bool:
    """Evaluate an equality comparison between two string expressions."""
    for operator, outcome in (("==", True), ("!=", False)):
        left, found, right = condition.partition(operator)
        if found:
            return (evaluate(left.strip(), bindings) == evaluate(right.strip(), bindings)) == outcome
    raise Unsupported(f"condition {condition!r}")


def evaluate_bind(expr: str, bindings: dict[str, str]):
    """Evaluate a `QVariant(...)` bind value to JSON."""
    expr = expr.strip()
    inner = re.fullmatch(r"QVariant[({](.*)[)}]", expr, re.S)
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
    if value == "QDate::currentDate()":
        return {"$ref": "current_date"}
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

VECTOR_DECL_RE = re.compile(r"^QVector<QueryAndParameters>\s*(?:const\s+)?(\w+)\s*(?=[{]|$)")
STRING_DECL_RE = re.compile(r"^QString\s*(?:const\s+)?(\w+)\s*(?:=\s*(.*)|\{(.*)\})?$", re.S)
STREAM_DECL_RE = re.compile(r"^QTextStream\s+(\w+)\s*\(\s*&(\w+)\s*\)$", re.S)
LIST_DECL_RE = re.compile(r"^std::vector<char const \*>\s*(?:const\s+)?(\w+)\s*\{(.*)\}$", re.S)
FOREACH_RE = re.compile(r"\s*(?:char const \*|auto)\s*(?:const\s*&?\s*|\*\s*const\s+)?(\w+)\s*:\s*(.*?)\s*$", re.S)
DESTRUCTURING_RE = re.compile(r"\s*auto\s+const\s*&?\s*\[([\w\s,]+)\]\s*:\s*\{(.*)\}\s*$", re.S)
RECORD_DECL_RE = re.compile(r"^QSqlRecord\s*(?:const\s+)?(\w+)\s*=\s*q\.driver\(\)->record\(\s*\"(\w+)\"\s*\)$", re.S)
CONTAINS_RE = re.compile(r"^(\w+)\.contains\((.*)\)$", re.S)

IGNORED_STATEMENT_RE = re.compile(r"^(qDebug|qInfo|qWarning|qCritical)\b")


def top_level_pieces(text: str):
    """Yield each top-level statement, and each `for (...) {...}` as a (header, body) pair."""
    depth, start, consumed = 0, 0, -1
    for index, char in scan(text):
        if index < consumed:
            continue
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == ";" and depth == 0:
            piece = text[start:index].strip()
            if piece:
                yield ("statement", piece)
            start = index + 1
        elif depth == 0 and char in "fi" and re.match(r"(for|if)\s*\(", text[index:]):
            keyword = "loop" if text[index] == "f" else "conditional"
            before = text[start:index].strip()
            if before:
                yield ("statement", before)
            header_start, header_end = match_balanced(text, index, "(", ")")
            body_start, body_end = match_balanced(text, header_end)
            yield (keyword, (text[header_start + 1 : header_end - 1], text[body_start + 1 : body_end - 1]))
            start = consumed = body_end
    tail = text[start:].strip()
    if tail:
        yield ("statement", tail)


class Walker:
    """Interprets the handful of C++ constructs the `migrate_to_*` functions are written in."""

    def __init__(self) -> None:
        self.values: dict[str, str] = {}  # QString locals
        self.lists: dict[str, list[str]] = {}  # std::vector<char const *> locals
        self.streams: dict[str, str] = {}  # QTextStream name -> the QString it writes into
        self.records: dict[str, str] = {}  # QSqlRecord local -> the table it describes
        self.condition: dict | None = None  # guard applied to statements appended inside an `if`
        self.vector: str | None = None  # the QVector<QueryAndParameters> being built
        self.statements: list[dict] = []

    def env(self, bindings: dict[str, str]) -> dict[str, str]:
        return {**self.values, **bindings}

    def run(self, text: str, bindings: dict[str, str]) -> None:
        for kind, piece in top_level_pieces(text):
            if kind == "loop":
                self.loop(*piece, bindings)
            elif kind == "conditional":
                self.conditional(*piece, bindings)
            else:
                self.statement(piece, bindings)

    def conditional(self, header: str, body: str, bindings: dict[str, str]) -> None:
        """Handle `if (record.contains(column)) { ... }`.

        Several migrations rename or drop a column that only exists in databases upgraded from an
        older schema -- one created fresh at that version never had it, and SQLite has no
        `DROP COLUMN IF EXISTS`.  The guard travels with the statement so the runner can apply it.
        """
        contains = CONTAINS_RE.fullmatch(header.strip())
        if not contains or contains.group(1) not in self.records:
            raise Unsupported(f"condition {header.strip()!r}")
        table = self.records[contains.group(1)]
        column = evaluate(contains.group(2), self.env(bindings))
        if self.condition is not None:
            raise Unsupported("nested conditionals")
        self.condition = {"column_exists": [table, column]}
        try:
            self.run(body, bindings)
        finally:
            self.condition = None

    def loop(self, header: str, body: str, bindings: dict[str, str]) -> None:
        destructuring = DESTRUCTURING_RE.fullmatch(header)
        if destructuring:
            names = [name.strip() for name in destructuring.group(1).split(",")]
            for item in split_arguments(destructuring.group(2)):
                values = split_arguments(re.sub(r"^std::pair\s*\{(.*)\}$", r"\1", item.strip(), flags=re.S))
                if len(values) != len(names):
                    raise Unsupported(f"destructuring {item!r} into {names}")
                extra = {name: evaluate(value, self.env(bindings)) for name, value in zip(names, values)}
                self.run(body, {**bindings, **extra})
            return

        match = FOREACH_RE.fullmatch(header)
        if not match:
            raise Unsupported(f"loop header {header!r}")
        variable, source = match.group(1), match.group(2).strip()
        if source in self.lists:
            items = self.lists[source]
        elif source.startswith("{"):
            items = [string_literals(item) for item in split_arguments(source[1:-1])]
        else:
            raise Unsupported(f"loop over {source!r}")
        if any(item is None for item in items):
            raise Unsupported("loop over non-literals")
        for item in items:
            self.run(body, {**bindings, variable: item})

    def statement(self, text: str, bindings: dict[str, str]) -> None:
        if text.startswith("return") or IGNORED_STATEMENT_RE.match(text):
            return

        vector = VECTOR_DECL_RE.match(text)
        if vector:
            self.vector = vector.group(1)
            if vector.end() >= len(text.rstrip()):
                return  # declared empty; the queries are appended below
            start, end = match_balanced(text, vector.end())
            self.statements += [parse_entry(e, self.env(bindings)) for e in split_top_level(text[start + 1 : end - 1])]
            return

        if self.vector and text.startswith(f"{self.vector}.append("):
            start, end = match_balanced(text, len(self.vector) + len(".append") - 1, "(", ")")
            entry = parse_entry(text[start + 1 : end - 1].strip(), self.env(bindings))
            if self.condition is not None:
                entry["condition"] = self.condition
            self.statements.append(entry)
            return

        record = RECORD_DECL_RE.match(text)
        if record:
            self.records[record.group(1)] = record.group(2)
            return

        stream = STREAM_DECL_RE.match(text)
        if stream:
            self.streams[stream.group(1)] = stream.group(2)
            return

        if "<<" in text and text.split("<<", 1)[0].strip() in self.streams:
            name, _, rest = text.partition("<<")
            target = self.streams[name.strip()]
            self.values[target] = self.values.get(target, "") + "".join(
                evaluate(part, self.env(bindings)) for part in split_shifts(rest)
            )
            return

        table = LIST_DECL_RE.match(text)
        if table:
            items = [string_literals(item) for item in split_arguments(table.group(2))]
            if any(item is None for item in items):
                raise Unsupported(f"non-literal entry in {table.group(1)}")
            self.lists[table.group(1)] = items
            return

        string = STRING_DECL_RE.match(text)
        if string:
            initialiser = string.group(2) or string.group(3)
            self.values[string.group(1)] = evaluate(initialiser, self.env(bindings)) if initialiser else ""
            return

        raise Unsupported(f"statement {text[:70]!r}")


def split_shifts(text: str) -> list[str]:
    """Split the right-hand side of a `QTextStream <<` chain on its top-level `<<` operators."""
    depth, cuts, skip = 0, [], -1
    for index, char in scan(text):
        if index == skip:
            continue
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "<" and depth == 0 and text.startswith("<<", index):
            cuts.append(index)
            skip = index + 1
    parts, previous = [], 0
    for cut in cuts:
        parts.append(text[previous:cut])
        previous = cut + 2
    parts.append(text[previous:])
    return [p.strip() for p in parts if p.strip()]


def extract_function(body: str) -> list[dict]:
    """Extract the statements one `migrate_to_*` function runs."""
    walker = Walker()
    walker.run(body, {})
    if walker.vector is None and not walker.statements:
        if re.search(r"return\s+true\s*;", body) and "executeSqlQueries" not in body:
            return []  # a deliberate no-op migration
        raise Unsupported("no QVector<QueryAndParameters> declaration")
    return walker.statements


# ── Entry point ───────────────────────────────────────────────────────────────────────────────

FUNCTION_RE = re.compile(r"^\s*bool (migrate_to_\w+)\(", re.M)

# `migrateNext` maps each starting schema version onto the function that advances it by one.  The
# first two are named after application versions rather than schema versions, hence the lookup.
DISPATCH_RE = re.compile(r"case\s+(\d+):\s*ret\s*&=\s*(migrate_to_\w+)\s*\(")


def function_bodies(text: str) -> dict[str, str]:
    """Carve `DatabaseSchemaHelper.cpp` into its `migrate_to_*` function bodies."""
    bodies, matches = {}, list(FUNCTION_RE.finditer(text))
    for index, match in enumerate(matches):
        limit = matches[index + 1].start() if index + 1 < len(matches) else len(text)
        # match.end() is just past the opening parenthesis, so step back onto it.
        _, end = match_balanced(text, match.end() - 1, "(", ")")
        body_start, body_end = match_balanced(text, end)
        if body_end > limit:
            raise Unsupported(f"{match.group(1)} body runs past the next function")
        bodies[match.group(1)] = text[body_start + 1 : body_end - 1]
    return bodies


def build() -> dict:
    raw = SRC.read_text(encoding="utf-8")
    text = strip_preprocessor_conditionals(strip_comments(raw))
    bodies = function_bodies(text)
    dispatch = [(int(version), name) for version, name in DISPATCH_RE.findall(text)]
    if not dispatch:
        raise Unsupported("could not find the migrateNext dispatch table")

    latest = int(re.search(r"DatabaseSchemaHelper::latestVersion\s*=\s*(\d+)", raw).group(1))
    migrations, unsupported = [], []
    for from_version, function in sorted(dispatch):
        entry = {"from_version": from_version, "to_version": from_version + 1, "cpp_function": function}
        try:
            entry["statements"] = extract_function(bodies[function])
        except (Unsupported, KeyError, ValueError) as error:
            entry["unsupported_reason"] = str(error)
            unsupported.append(f"{function} (v{from_version} -> v{from_version + 1}): {error}")
        migrations.append(entry)

    return {
        "latest_version": latest,
        "generated_by": "tools/extract-migrations.py",
        "source_files": [str(SRC.relative_to(REPO))],
        "migrations": migrations,
        "hand_ported": [m["cpp_function"] for m in migrations if "statements" not in m],
    }, unsupported


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    parser.add_argument("--summary", action="store_true", help="print a human-readable summary")
    args = parser.parse_args()

    document, unsupported = build()
    if args.summary:
        for migration in document["migrations"]:
            count = len(migration.get("statements", []))
            state = f"{count:>3} statements" if "statements" in migration else "HAND PORT"
            print(f"v{migration['from_version']:>2} -> v{migration['to_version']:<3} {migration['cpp_function']:<16} {state}")
        if unsupported:
            print("\nmust be hand-ported in Rust:")
            for line in unsupported:
                print(f"  {line}")
        return 0

    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
        total = sum(len(m.get("statements", [])) for m in document["migrations"])
        print(f"wrote {args.out} ({len(document['migrations'])} migrations, {total} statements, "
              f"{len(document['hand_ported'])} to hand-port)")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
