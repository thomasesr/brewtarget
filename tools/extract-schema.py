#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-schema.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract the canonical Brewtarget database schema from the legacy Qt/C++ tree.

The schema is declared, not written out, in ``src/database/ObjectStoreTyped.cpp`` as
specialisations of ``PRIMARY_TABLE<T>``.  Hand-transcribing it would be error-prone and would
silently drift, so this script parses those declarations and emits ``schema/v23.json``, which is
the single input to the Rust DDL renderer and to the generated model structs.

See PLAN.md Phase 1.  Run:

    python3 tools/extract-schema.py --out schema/v23.json
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
SRC_TYPED = REPO / "src" / "database" / "ObjectStoreTyped.cpp"
SRC_SCHEMA_HELPER = REPO / "src" / "database" / "DatabaseSchemaHelper.cpp"


# ── C++ source handling ───────────────────────────────────────────────────────────────────────

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


# ── Field parsing ─────────────────────────────────────────────────────────────────────────────

FIELD_TYPE_RE = re.compile(r"ObjectStore::FieldType::(\w+)")
PROPERTY_RE = re.compile(r"PropertyNames::([\w:]+)::(\w+)")
COLUMN_RE = re.compile(r'"([A-Za-z_][A-Za-z_0-9]*)"')


def parse_fk_target(entry: str) -> str | None:
    """Return the C++ type a `&PRIMARY_TABLE<...>` reference points at, if any."""
    marker = "&PRIMARY_TABLE<"
    pos = entry.find(marker)
    if pos < 0:
        return None
    start = pos + len(marker) - 1
    s, e = match_balanced(entry, start, "<", ">")
    return entry[s + 1 : e - 1].strip()


def parse_mapping(entry: str) -> str | None:
    """Return the enum/unit string-mapping reference (e.g. `&Hop::formStringMapping`), if any."""
    for m in re.finditer(r"&([A-Za-z_][\w:]*)", entry):
        name = m.group(1)
        if not name.startswith("PRIMARY_TABLE"):
            return name
    return None


def parse_fields(fields_body: str) -> list[dict]:
    fields = []
    for entry in split_top_level(fields_body[1:-1]):
        ft = FIELD_TYPE_RE.search(entry)
        if not ft:
            continue
        cols_s, cols_e = match_balanced(entry, ft.end())
        columns = COLUMN_RE.findall(entry[cols_s:cols_e])
        prop = PROPERTY_RE.search(entry)
        fields.append(
            {
                "field_type": ft.group(1),
                "columns": columns,
                "property": f"{prop.group(1)}::{prop.group(2)}" if prop else None,
                "foreign_key_to_type": parse_fk_target(entry),
                "string_mapping": parse_mapping(entry),
            }
        )
    return fields


def _unwrap(fields_body: str) -> str:
    """`*_COMMON_FIELDS` vectors wrap their entries in one extra pair of braces; strip it.

    A field entry's interior starts with `ObjectStore::FieldType::...`, whereas a wrapper's
    interior starts with the opening brace of the first entry.  That is what tells them apart.
    """
    groups = split_top_level(fields_body[1:-1])
    if len(groups) == 1 and groups[0][1:-1].lstrip().startswith("{"):
        return groups[0]
    return fields_body


# ── Table parsing ─────────────────────────────────────────────────────────────────────────────

COMMON_SET_RE = re.compile(r"&(\w+_COMMON_FIELDS)")


def expand_folder_macro(text: str) -> str:
    """`TABLE_FOLDER_DEFN` generates eleven structurally identical `folder_for_*` tables."""
    define = text.find("#define TABLE_FOLDER_DEFN(BaseName, baseName)")
    if define < 0:
        return text
    end = define
    while True:
        line_end = text.find("\n", end)
        if line_end < 0 or not text[end:line_end].rstrip().endswith("\\"):
            end = line_end if line_end > 0 else len(text)
            break
        end = line_end + 1
    body = text[define:end].split(")", 1)[1].replace("\\\n", "\n")
    expanded = []
    for base, lower in re.findall(r"^\s*TABLE_FOLDER_DEFN\(\s*(\w+)\s*,\s*(\w+)\s*\)", text, re.M):
        one = body.replace('"folder_for_" #baseName', f'"folder_for_{lower}"')
        expanded.append(re.sub(r"\bBaseName\b", base, one))
    return text[:define] + "\n".join(expanded) + text[end:]


def parse_common_sets(text: str) -> dict[str, list[dict]]:
    sets = {}
    for m in re.finditer(r"std::vector<ObjectStore::TableField>\s+const\s+(\w+)\s*(?=\{)", text):
        s, e = match_balanced(text, m.end())
        sets[m.group(1)] = parse_fields(_unwrap(text[s:e]))
    return sets


def parse_primary_tables(text: str) -> list[dict]:
    tables = []
    marker = "ObjectStore::TableDefinition const PRIMARY_TABLE<"
    pos = 0
    while True:
        pos = text.find(marker, pos)
        if pos < 0:
            return tables
        ts, te = match_balanced(text, pos + len(marker) - 1, "<", ">")
        cpp_type = text[ts + 1 : te - 1].strip()
        pos = te
        if cpp_type == "NE":  # the deleted generic primary template, not a real table
            continue
        bs, be = match_balanced(text, te)
        body = text[bs:be]
        name_match = COLUMN_RE.search(body)
        groups = split_top_level(body[1:-1])
        if not name_match or not groups:
            continue
        tables.append(
            {
                "cpp_type": cpp_type,
                "name": name_match.group(1),
                "own_fields": parse_fields(groups[0]),
                "common_sets": COMMON_SET_RE.findall(groups[1]) if len(groups) > 1 else [],
            }
        )
        pos = be


# ── Type mapping ──────────────────────────────────────────────────────────────────────────────
#
# Mirrors ObjectStore.cpp `getBaseFieldType` / `getDatabaseNativeTypeName` and Database.cpp
# `DbNativeVariants`.  The sqlite and postgresql columns must stay byte-identical to the C++;
# the mariadb column is new (the C++ supports only two backends) and is marked as such.

BASE_TYPE_OF = {
    "Bool": ["bool"],
    "Int": ["int"],
    "UInt": ["uint"],
    "Double": ["double"],
    "String": ["string"],
    "Date": ["date"],
    "Enum": ["string"],  # stored as a string in the DB
    "Unit": ["string"],  # stored as a string in the DB
    "Money": ["string", "int"],  # currency code, then amount
}

NATIVE_TYPES = {
    #          sqlite     postgresql          mariadb
    "bool": ["BOOLEAN", "BOOLEAN", "BOOLEAN"],
    "int": ["INTEGER", "INTEGER", "INT"],
    "uint": ["INTEGER", "INTEGER", "INT"],
    "double": ["DOUBLE", "DOUBLE PRECISION", "DOUBLE"],
    "string": ["TEXT", "TEXT", "TEXT"],
    "date": ["DATE", "DATE", "DATE"],
}

DIALECTS = ["sqlite", "postgresql", "mariadb"]

PRIMARY_KEY_DECLARATION = {
    "sqlite": "INTEGER PRIMARY KEY",  # SQLite recommends against AUTOINCREMENT here
    "postgresql": "SERIAL PRIMARY KEY",
    "mariadb": "INT AUTO_INCREMENT PRIMARY KEY",
}

# Foreign keys are added after every table exists, so table creation order does not matter.
ADD_FOREIGN_KEY = {
    "sqlite": "ALTER TABLE {table} ADD COLUMN {column} INTEGER REFERENCES {ref_table}({ref_column});",
    "postgresql": "ALTER TABLE {table} ADD COLUMN {column} INTEGER REFERENCES {ref_table}({ref_column});",
    # The C++ carries this variant only as a comment in Database.cpp; its `ADD COLUMN x int, FOREIGN
    # KEY ...` form is not valid MySQL/MariaDB, which needs a second ADD.  Corrected here.
    "mariadb": "ALTER TABLE {table} ADD COLUMN {column} INT, ADD FOREIGN KEY ({column}) REFERENCES {ref_table}({ref_column});",
}

IDENTIFIER_QUOTE = {"sqlite": '"', "postgresql": '"', "mariadb": "`"}


def resolve(tables: list[dict], common_sets: dict[str, list[dict]]) -> list[dict]:
    """Flatten own + common fields into ordered columns, splitting out primary and foreign keys."""
    by_type = {t["cpp_type"]: t["name"] for t in tables}
    resolved = []
    for table in tables:
        fields = [dict(f, from_common_set=None) for f in table["own_fields"]]
        for set_name in table["common_sets"]:
            if set_name not in common_sets:
                raise KeyError(f"{table['name']}: unknown common field set {set_name}")
            fields += [dict(f, from_common_set=set_name) for f in common_sets[set_name]]

        pk = fields[0]
        if pk["field_type"] != "Int" or len(pk["columns"]) != 1:
            raise ValueError(f"{table['name']}: primary key must be a single Int column")

        columns, foreign_keys = [], []
        for field in fields[1:]:
            if field["foreign_key_to_type"]:
                target = field["foreign_key_to_type"]
                if target not in by_type:
                    raise KeyError(f"{table['name']}.{field['columns'][0]}: unknown FK target {target}")
                foreign_keys.append(
                    {
                        "column": field["columns"][0],
                        "references_table": by_type[target],
                        "references_column": "id",
                        "property": field["property"],
                        "from_common_set": field["from_common_set"],
                    }
                )
                continue
            base_types = BASE_TYPE_OF[field["field_type"]]
            for index, column in enumerate(field["columns"]):
                columns.append(
                    {
                        "name": column,
                        "base_type": base_types[index],
                        "field_type": field["field_type"],
                        "property": field["property"],
                        "string_mapping": field["string_mapping"],
                        "from_common_set": field["from_common_set"],
                    }
                )

        resolved.append(
            {
                "name": table["name"],
                "cpp_type": table["cpp_type"],
                "primary_key": {"name": pk["columns"][0], "property": pk["property"]},
                "columns": columns,
                "foreign_keys": foreign_keys,
            }
        )
    return resolved


# ── Entry point ───────────────────────────────────────────────────────────────────────────────

def schema_version() -> int:
    text = SRC_SCHEMA_HELPER.read_text(encoding="utf-8")
    m = re.search(r"DatabaseSchemaHelper::latestVersion\s*=\s*(\d+)", text)
    if not m:
        raise ValueError(f"could not find latestVersion in {SRC_SCHEMA_HELPER}")
    return int(m.group(1))


def build() -> dict:
    raw = SRC_TYPED.read_text(encoding="utf-8")
    text = expand_folder_macro(strip_comments(raw))
    common_sets = parse_common_sets(text)
    tables = resolve(parse_primary_tables(text), common_sets)
    tables.sort(key=lambda t: t["name"])
    return {
        "schema_version": schema_version(),
        "generated_by": "tools/extract-schema.py",
        "source_files": [
            str(SRC_TYPED.relative_to(REPO)),
            str(SRC_SCHEMA_HELPER.relative_to(REPO)),
        ],
        "dialects": DIALECTS,
        "native_types": {base: dict(zip(DIALECTS, names)) for base, names in NATIVE_TYPES.items()},
        "primary_key_declaration": PRIMARY_KEY_DECLARATION,
        "add_foreign_key": ADD_FOREIGN_KEY,
        "identifier_quote": IDENTIFIER_QUOTE,
        "tables": tables,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    parser.add_argument("--summary", action="store_true", help="print a human-readable table summary")
    args = parser.parse_args()

    schema = build()
    if args.summary:
        for table in schema["tables"]:
            print(f"{table['name']:<34} {len(table['columns']) + 1:>3} columns  {len(table['foreign_keys']):>2} FKs")
        print(f"\n{len(schema['tables'])} tables, schema version {schema['schema_version']}")
        return 0

    text = json.dumps(schema, indent=2, sort_keys=False) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
        print(f"wrote {args.out} ({len(schema['tables'])} tables, schema v{schema['schema_version']})")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
