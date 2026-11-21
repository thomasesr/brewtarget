#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-enums.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract the enum-to-string mappings the database stores.

Columns of `ObjectStore::FieldType::Enum` hold a string, not a number, and the mapping between the
two is declared in `src/model/` as `EnumStringMapping const Class::nameStringMapping`.  Those
strings are on-disk data: getting one wrong corrupts a column silently and is only noticed when
the C++ application refuses to read the row back.

    python3 tools/extract-enums.py --out schema/enums.json

See PLAN.md Phase 4.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

from cpputil import match_balanced, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
# BeerXML declares its own mappings -- it supports fewer hop forms and malt types than we do,
# so it needs its own names for them -- which is why src/serialization is scanned too.
SOURCE_DIRS = ["src/model", "src/measurement", "src/serialization", "src/calculations"]

# Model classes declare theirs as `Owner::nameStringMapping`; the serialization code declares
# file-scope ones with no owner at all.
MAPPING_RE = re.compile(r"EnumStringMapping const (?:([\w:]+?)::)?(\w+)\s*(?=\{)")
# Unit columns store a unit's name rather than an enum member, so their mapping is by object
# address rather than by enum value.  Same job, different declaration.
OBJECT_MAPPING_RE = re.compile(r"ObjectAddressStringMapping<(\w+)> const (\w+)\s*(?=\{)")
ENTRY_RE = re.compile(r'\{\s*([\w:]+)\s*,\s*"((?:[^"\\]|\\.)*)"\s*\}')
ENUM_DECL_RE = re.compile(r"enum class (\w+)\s*(?::[^{]*)?(?=\{)")


def source_files() -> list[pathlib.Path]:
    files = []
    for directory in SOURCE_DIRS:
        files += sorted((REPO / directory).rglob("*.cpp"))
        files += sorted((REPO / directory).rglob("*.h"))
    return files


def parse_enum_members(text: str, owner: str) -> dict[str, list[str]]:
    """Read every `enum class` in a header, keeping its members in declaration order.

    Keyed by owning class, because `Form` means one thing on `Hop` and another on `Yeast`.
    """
    enums = {}
    for match in ENUM_DECL_RE.finditer(text):
        start, end = match_balanced(text, match.end())
        members = []
        for entry in text[start + 1 : end - 1].split(","):
            member = re.match(r"\s*(\w+)", entry)
            if member:
                members.append(member.group(1))
        enums[f"{owner}::{match.group(1)}"] = members
    return enums


def build() -> dict:
    mappings = {}
    enums: dict[str, list[str]] = {}
    for path in source_files():
        text = strip_comments(path.read_text(encoding="utf-8"))
        enums.update(parse_enum_members(text, path.stem))
        for match in OBJECT_MAPPING_RE.finditer(text):
            start, end = match_balanced(text, match.end())
            values = [
                {"member": member.split("::")[-1], "string": string}
                for member, string in ENTRY_RE.findall(text[start + 1 : end - 1])
            ]
            if values:
                namespace = "Measurement::Units" if match.group(1) == "Unit" else match.group(1)
                mappings[f"{namespace}::{match.group(2)}"] = {
                    "owner": namespace,
                    "name": match.group(2),
                    "enum": match.group(1),
                    "source_file": str(path.relative_to(REPO)),
                    "by_object_address": True,
                    "values": values,
                }

        for match in MAPPING_RE.finditer(text):
            owner, name = match.group(1) or "", match.group(2)
            start, end = match_balanced(text, match.end())
            entries = []
            for member, string in ENTRY_RE.findall(text[start + 1 : end - 1]):
                # Entries are written `{Hop::Form::Leaf, "leaf"}`; the enum is the last qualifier.
                parts = member.split("::")
                entries.append({"member": parts[-1], "enum": "::".join(parts[:-1]), "string": string})
            if not entries:
                continue
            mappings[f"{owner}::{name}" if owner else name] = {
                "owner": owner,
                "name": name,
                "enum": entries[0]["enum"],
                "source_file": str(path.relative_to(REPO)),
                "values": [{"member": e["member"], "string": e["string"]} for e in entries],
            }

    # Cross-check each mapping against its enum declaration.  Entries are written with varying
    # qualification depending on the file, so match on the final component of the name.
    inconsistent = []
    for name, mapping in mappings.items():
        if mapping.get("by_object_address"):
            continue
        leaf = mapping["enum"].split("::")[-1]
        declarations = [members for key, members in enums.items() if key.split("::")[-1] == leaf]
        if not declarations:
            continue
        declared = {member for members in declarations for member in members}
        unknown = [value["member"] for value in mapping["values"] if value["member"] not in declared]
        if unknown:
            inconsistent.append(f"{name}: members not declared in enum {leaf}: {unknown}")

    # Every mapping the schema refers to must have been found, or an Enum column would have no
    # way of being read back.
    schema = json.loads((REPO / "schema" / "v23.json").read_text(encoding="utf-8"))
    referenced = sorted({c["string_mapping"] for t in schema["tables"] for c in t["columns"] if c["string_mapping"]})
    missing = [name for name in referenced if name not in mappings]

    return {
        "generated_by": "tools/extract-enums.py",
        "source_dirs": SOURCE_DIRS,
        "mappings": dict(sorted(mappings.items())),
        "enum_members": {name: members for name, members in sorted(enums.items())},
        "referenced_by_schema": referenced,
        "missing": missing,
        "inconsistent": inconsistent,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    parser.add_argument("--summary", action="store_true", help="print a human-readable summary")
    args = parser.parse_args()

    document = build()
    if args.summary:
        for name, mapping in document["mappings"].items():
            strings = ", ".join(value["string"] for value in mapping["values"])
            print(f"{name:<52} {strings[:70]}")
        print(f"\n{len(document['mappings'])} mappings, {len(document['referenced_by_schema'])} referenced by the schema")
        if document["missing"]:
            print(f"MISSING: {document['missing']}")
        return 1 if document["missing"] else 0

    if document["missing"]:
        print(f"error: schema columns refer to mappings that were not found: {document['missing']}", file=sys.stderr)
        return 1
    if document["inconsistent"]:
        for problem in document["inconsistent"]:
            print(f"error: {problem}", file=sys.stderr)
        return 1

    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
        print(f"wrote {args.out} ({len(document['mappings'])} mappings)")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
