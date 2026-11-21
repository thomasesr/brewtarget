#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-formats.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract the file format field mappings from the legacy Qt/C++ tree.

Brewtarget reads and writes three interchange formats -- BeerXML, BeerJSON and `.beer` -- and
none of them lines up with the database schema.  The mapping between each format's field names
and the model's properties is declared in `src/serialization/`, a few hundred entries per format,
and getting one wrong means quietly losing a field on import or writing a file other software
cannot read.

    python3 tools/extract-formats.py --out schema/formats.json

See PLAN.md Phase 6.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

from cpputil import match_balanced, scan, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
SOURCES = {
    "dotBeer": REPO / "src" / "serialization" / "json" / "dotBeer" / "DotBeer.cpp",
    "beerJson": REPO / "src" / "serialization" / "json" / "beerJson" / "BeerJson.cpp",
    "beerXml": REPO / "src" / "serialization" / "xml" / "BeerXml.cpp",
}


class Unsupported(Exception):
    """Raised when a declaration is not of a form this script can lift."""


def split_arguments(text: str) -> list[str]:
    """Split an initialiser list on its top-level commas, ignoring string literals."""
    depth, cuts = 0, []
    for index, char in scan(text):
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == "," and depth == 0:
            cuts.append(index)
    pieces, previous = [], 0
    for cut in cuts:
        pieces.append(text[previous:cut])
        previous = cut + 1
    pieces.append(text[previous:])
    return [piece.strip() for piece in pieces if piece.strip()]


STRING_LITERAL_RE = re.compile(r'"((?:[^"\\]|\\.)*)"')
FIELD_TYPE_RE = re.compile(r"(?:Json|Xml)RecordDefinition::FieldType::(\w+)")
PROPERTY_RE = re.compile(r"PropertyNames::([\w:]+)::(\w+)")
# Field lists are declared as `std::initializer_list<...FieldDefinition> const NAME {`.
FIELD_LIST_RE = re.compile(r"std::initializer_list<(?:Json|Xml)RecordDefinition::FieldDefinition>\s*const\s+(\w+)\s*(?=\{)")


def string_literals(text: str) -> str | None:
    """Join adjacent C++ string literals, as the compiler would."""
    pieces = STRING_LITERAL_RE.findall(text)
    if not pieces or STRING_LITERAL_RE.sub("", text).strip():
        return None
    return "".join(pieces).encode().decode("unicode_escape")


def parse_field(entry: str) -> dict | None:
    """Parse one `{FieldType::X, "path", PropertyNames::A::b, &decoder}` entry."""
    field_type = FIELD_TYPE_RE.search(entry)
    if not field_type:
        return None
    arguments = split_arguments(entry[1:-1])
    # The path is the first pure string literal after the field type.  It may be empty: a Record
    # field with no path of its own is read from the record it sits in rather than a child object.
    path = None
    for argument in arguments[1:]:
        literal = string_literals(argument)
        if literal is not None:
            path = literal
            break
    if path is None:
        raise Unsupported(f"no path in {entry!r}")
    property_name = PROPERTY_RE.search(entry)
    # A trailing `&something` is the value decoder: an enum mapping, a unit mapping, or a
    # reference to another record definition for a nested object.  `staticMetaObject` is Qt
    # plumbing rather than a decoder.
    decoder = None
    # The template argument may be padded for alignment -- `<Fermentable >` -- so allow spaces
    # inside the angle brackets and strip them afterwards.
    for candidate in re.findall(r"&\s*([\w:]+(?:<[\w:<>\s]*>)?)", entry):
        if candidate.endswith("::staticMetaObject"):
            continue
        decoder = re.sub(r"\s+", "", candidate)
        break
    nested = re.fullmatch(r"\w*RECORD_DEFN(?:<([\w:<>]+)>|_(\w+))", decoder or "")
    return {
        "field_type": field_type.group(1),
        "path": path,
        "property": f"{property_name.group(1)}::{property_name.group(2)}" if property_name else None,
        "decoder": decoder,
        # A Record or ListOfRecords field points at the definition of whatever it contains.
        "nested_record_type": (nested.group(1) or decoder) if nested else None,
        # Some fields carry a validator rather than a decoder -- a style letter, for instance.
        "validator": decoder if decoder and decoder.endswith("_VALIDATOR") else None,
    }


UNITS_MAPPING_RE = re.compile(r"JsonMeasureableUnitsMapping const (\w+)\s*(?=\{)")
LIST_OF_UNITS_RE = re.compile(r"ListOfJsonMeasureableUnitsMappings const (\w+)\s*(?=\{)")
SINGLE_UNIT_RE = re.compile(r"JsonSingleUnitSpecifier const (\w+)\s*\{\s*\{([^}]*)\}\s*\}")
UNIT_ENTRY_RE = re.compile(r'\{\s*"((?:[^"\\]|\\.)*)"\s*,\s*&(?:Measurement::)?Units::(\w+)\s*\}')


def parse_unit_mappings(text: str) -> dict[str, dict]:
    """Read the tables that map a format's unit names onto ours.

    A measurement in BeerJSON is a value and a unit together, so reading one means knowing that
    its "kg" is our kilograms.  A single unit specifier is the degenerate case: a field that is
    always in one unit, named so it can be written back out.
    """
    mappings: dict[str, dict] = {}
    for match in UNITS_MAPPING_RE.finditer(text):
        start, end = match_balanced(text, match.end())
        units = dict(UNIT_ENTRY_RE.findall(text[start + 1 : end - 1]))
        if units:
            mappings[match.group(1)] = {"kind": "units", "units": units}
    for match in LIST_OF_UNITS_RE.finditer(text):
        # A field that may be given as a mass or as a volume -- an ingredient amount, typically --
        # accepts any unit from several tables at once.
        start, end = match_balanced(text, match.end())
        referenced = re.findall(r"&(\w+)", text[start + 1 : end - 1])
        if referenced:
            mappings[match.group(1)] = {"kind": "any_of", "mappings": referenced}
    for match in SINGLE_UNIT_RE.finditer(text):
        names = STRING_LITERAL_RE.findall(match.group(2))
        if names:
            mappings[match.group(1)] = {"kind": "single_unit", "names": names}
    return mappings


# Most records are declared as a template specialisation naming the entity they store; a few --
# the cut-down ones used for an ingredient embedded in a recipe addition -- have a plain name.
RECORD_DEFN_RE = re.compile(r"(?:Json|Xml)RecordDefinition const (\w+)(?:<([\w:<>]+)>)?\s*(?=\{)")


def parse_field_lists(text: str) -> dict[str, list[dict]]:
    """Read every named list of field definitions."""
    lists = {}
    for match in FIELD_LIST_RE.finditer(text):
        start, end = match_balanced(text, match.end())
        fields = []
        for entry in split_arguments(text[start + 1 : end - 1]):
            if not entry.startswith("{"):
                continue
            field = parse_field(entry)
            if field:
                fields.append(field)
        lists[match.group(1)] = fields
    return lists


def infer_cpp_type(fields: list[dict]) -> str | None:
    """Work out which model class a record stores, from the properties its fields feed."""
    owners: dict[str, int] = {}
    for field in fields:
        if not field["property"]:
            continue
        owner = field["property"].rsplit("::", 1)[0]
        # Properties shared by everything say nothing about which class this is.
        if owner in {"NamedEntity", "FolderPropertyBase", "IngredientAmount", "RecipeAddition"}:
            continue
        owners[owner] = owners.get(owner, 0) + 1
    if not owners:
        return None
    return max(owners.items(), key=lambda entry: entry[1])[0]


def parse_records(text: str, field_lists: dict[str, list[dict]]) -> list[dict]:
    """Read the record definitions, resolving the field lists each one is built from.

    A record is declared as the entity it stores, the name it goes by in the file, and the lists
    of fields it is made of -- the shared ones first, then its own.
    """
    records = []
    for match in RECORD_DEFN_RE.finditer(text):
        start, end = match_balanced(text, match.end())
        body = text[start + 1 : end - 1]
        arguments = split_arguments(body)
        if len(arguments) < 2:
            continue

        if match.group(2) is None and match.group(1).endswith("RECORD_DEFN"):
            continue  # the generic primary template, which is never used directly
        record_name = None
        for argument in arguments:
            literal = string_literals(argument)
            if literal is not None:
                record_name = literal
                break
        if record_name is None:
            continue

        # The last argument is a braced list of the field lists to concatenate.
        referenced, fields = [], []
        for argument in reversed(arguments):
            if argument.startswith("{"):
                referenced = [name.strip() for name in split_arguments(argument[1:-1])]
                break
        # An element is either the name of a shared list or, for the records that do not share
        # anything, a field definition written out in place.
        inline = []
        names = []
        for element in referenced:
            if element.startswith("{"):
                field = parse_field(element)
                if field:
                    inline.append(field)
            else:
                names.append(element)
        for name in names:
            if name not in field_lists:
                raise Unsupported(f"{match.group(2)}: unknown field list {name}")
            fields += field_lists[name]
        fields += inline
        referenced = names

        records.append(
            {
                "name": match.group(1),
                # A definition declared without a template parameter does not say what it stores,
                # so take it from what its fields feed: the embedded hop in a BeerXML recipe
                # addition has fields on Hop, and that is what makes it a hop.
                "cpp_type": match.group(2) or infer_cpp_type(fields) or match.group(1),
                "record_name": record_name,
                "field_lists": referenced,
                "fields": fields,
            }
        )
    return records


def build() -> dict:
    formats = {}
    for name, path in SOURCES.items():
        text = strip_comments(path.read_text(encoding="utf-8"))
        field_lists = parse_field_lists(text)
        records = parse_records(text, field_lists)
        formats[name] = {
            "source_file": str(path.relative_to(REPO)),
            "unit_mappings": parse_unit_mappings(text),
            "field_lists": field_lists,
            "records": records,
        }

    # Every property a format maps must exist somewhere in the schema, or a field would be read
    # into nothing.  Reported rather than enforced: some properties are calculated and are
    # deliberately written out without being stored.
    schema = json.loads((REPO / "schema" / "v23.json").read_text(encoding="utf-8"))
    stored = {
        column["property"] for table in schema["tables"] for column in table["columns"] if column["property"]
    } | {table["primary_key"]["property"] for table in schema["tables"] if table["primary_key"]["property"]}

    unstored = sorted(
        {
            field["property"]
            for format_data in formats.values()
            for record in format_data["records"]
            for field in record["fields"]
            if field["property"] and field["property"] not in stored
        }
    )

    # Every decoder a field names must resolve to something: a unit mapping, an enum mapping, a
    # nested record, or a validator.  One that does not means a field would be read as raw text.
    enums = json.loads((REPO / "schema" / "enums.json").read_text(encoding="utf-8"))["mappings"]
    enum_names = set(enums) | {name.split("::")[-1] for name in enums}
    unresolved = []
    for name, format_data in formats.items():
        known_records = {record["cpp_type"] for record in format_data["records"]} | {
            record["name"] for record in format_data["records"]
        }
        for record in format_data["records"]:
            for field in record["fields"]:
                decoder = field["decoder"]
                if not decoder or field["validator"]:
                    continue
                if field["nested_record_type"]:
                    if field["nested_record_type"] not in known_records:
                        unresolved.append(f"{name}/{record['record_name']}: no record {field['nested_record_type']}")
                    continue
                if decoder in format_data["unit_mappings"]:
                    continue
                if decoder in enum_names or decoder.split("::")[-1] in enum_names:
                    continue
                unresolved.append(f"{name}/{record['record_name']}/{field['path']}: unknown decoder {decoder}")

    return {
        "generated_by": "tools/extract-formats.py",
        "formats": formats,
        "properties_not_stored_in_the_schema": unstored,
        "unresolved_decoders": unresolved,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    parser.add_argument("--summary", action="store_true", help="print a human-readable summary")
    args = parser.parse_args()

    document = build()
    if document["unresolved_decoders"] and not args.summary:
        for problem in document["unresolved_decoders"]:
            print(f"error: {problem}", file=sys.stderr)
        return 1
    if args.summary:
        for name, format_data in document["formats"].items():
            total = sum(len(record["fields"]) for record in format_data["records"])
            print(
                f"{name:<10} {len(format_data['records']):>3} records, {total:>4} field mappings, "
                f"{len(format_data['unit_mappings']):>2} unit mappings"
            )
            for record in format_data["records"]:
                print(f"    {record['record_name']:<24} {record['cpp_type']:<28} {len(record['fields']):>3} fields")
        print(f"\n{len(document['properties_not_stored_in_the_schema'])} properties not stored in the schema")
        if document["unresolved_decoders"]:
            print(f"UNRESOLVED: {document['unresolved_decoders']}")
        return 1 if document["unresolved_decoders"] else 0

    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
        total = sum(
            len(record["fields"]) for format_data in document["formats"].values() for record in format_data["records"]
        )
        print(f"wrote {args.out} ({total} field mappings across {len(document['formats'])} formats)")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
