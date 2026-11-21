#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-units.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract the units of measurement from the legacy Qt/C++ tree.

``src/measurement/Unit.cpp`` declares around eighty units, most of them as a scale factor relative
to a canonical unit -- `0.45359237` kilograms to the pound, `3.785411784` litres to the US gallon,
and so on.  Those constants are exactly the kind of thing that a transcription error would corrupt
silently, so lift them rather than retype them.

A handful of units convert by formula rather than by scale: Fahrenheit, Lovibond, gravity points,
Plato, Brix and Windisch-Kolbach.  Those are written as lambdas in the C++ and are reported here as
`converts_by_formula` for `bt-measure` to implement in Rust.

    python3 tools/extract-units.py --out schema/units.json

See PLAN.md Phase 3.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

# Python puts this script's directory on sys.path, so this resolves to tools/cpputil.py.
from cpputil import match_balanced, scan, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
SRC_UNIT = REPO / "src" / "measurement" / "Unit.cpp"
SRC_UNIT_SYSTEM = REPO / "src" / "measurement" / "UnitSystem.cpp"
SRC_PHYSICAL_QUANTITY = REPO / "src" / "measurement" / "PhysicalQuantity.h"


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
    return [p.strip() for p in pieces if p.strip()]


ARITHMETIC_RE = re.compile(r"^[\d\s./*+()-]+$")


def number(expr: str) -> float:
    """Evaluate the small arithmetic expressions the scale factors are written as."""
    expr = expr.strip()
    if not ARITHMETIC_RE.fullmatch(expr):
        raise Unsupported(f"not a number: {expr!r}")
    return float(eval(expr, {"__builtins__": {}}, {}))  # noqa: S307 - input is digits and operators only


# ── Units ─────────────────────────────────────────────────────────────────────────────────────

UNIT_DECL_RE = re.compile(r"^\s*Unit const (\w+)\s*(?=\{)", re.M)
TR_RE = re.compile(r'QObject::tr\(\s*"((?:[^"\\]|\\.)*)"\s*\)')
QUALIFIED_RE = re.compile(r"&?(?:Measurement::)?(?:Units|UnitSystems)::(\w+)")
BARE_REFERENCE_RE = re.compile(r"&\s*(\w+)\s*$")


def reference(expr: str) -> str:
    """Resolve `&Measurement::Units::kilograms` or, within the namespace, plain `&kilograms`."""
    qualified = QUALIFIED_RE.search(expr)
    if qualified:
        return qualified.group(1)
    bare = BARE_REFERENCE_RE.search(expr.strip())
    if not bare:
        raise Unsupported(f"not a reference: {expr!r}")
    return bare.group(1)


def parse_units(text: str) -> list[dict]:
    """Parse the `Unit const name{...}` declarations in `Unit.cpp`."""
    units = []
    for match in UNIT_DECL_RE.finditer(text):
        start, end = match_balanced(text, match.end())
        arguments = split_arguments(text[start + 1 : end - 1])
        if len(arguments) < 3:
            raise Unsupported(f"{match.group(1)} has {len(arguments)} arguments")

        unit = {
            "name": match.group(1),
            "unit_system": reference(arguments[0]),
            "abbreviations": TR_RE.findall(arguments[1]),
            # A sanitiser rewrites what the user typed before it is parsed; only specific gravity
            # has one, and it is reimplemented in Rust.
            "sanitises_input": "defaultSanitiser" not in arguments[2],
            "canonical": None,
            "scale": 1.0,
            "converts_by_formula": False,
            "boundary_value": None,
        }

        # The remaining arguments are either `scale, &canonical [, boundary]` or
        # `toCanonical, fromCanonical, &canonical`, the latter written as lambdas.
        rest = arguments[3:]
        if any("[](" in argument for argument in rest):
            unit["converts_by_formula"] = True
            rest = [argument for argument in rest if "[](" not in argument]
        elif rest and not rest[0].lstrip().startswith("&"):
            unit["scale"] = number(rest.pop(0))

        if rest and rest[0].lstrip().startswith("&"):
            unit["canonical"] = reference(rest.pop(0))
        if rest:
            unit["boundary_value"] = number(rest.pop(0))
        if rest:
            raise Unsupported(f"{unit['name']} has unexpected arguments {rest}")
        units.append(unit)
    return units


# ── Unit systems and physical quantities ──────────────────────────────────────────────────────

UNIT_SYSTEM_DECL_RE = re.compile(r"^\s*UnitSystem const (\w+)\s*(?=\{)", re.M)


def parse_unit_systems(text: str) -> list[dict]:
    """Parse the `UnitSystem const name{...}` declarations in `UnitSystem.cpp`."""
    systems = []
    for match in UNIT_SYSTEM_DECL_RE.finditer(text):
        start, end = match_balanced(text, match.end())
        arguments = split_arguments(text[start + 1 : end - 1])
        if len(arguments) < 3:
            raise Unsupported(f"{match.group(1)} has {len(arguments)} arguments")

        systems.append(
            {
                "name": match.group(1),
                "physical_quantity": arguments[0].split("::")[-1].strip(),
                "default_unit": reference(arguments[1]),
                "unique_name": TR_RE.sub(r"\1", arguments[2]).strip().strip('"'),
                "system_of_measurement": arguments[3].split("::")[-1].strip() if len(arguments) > 3 else "UniversalStandard",
                # Smallest to largest, which is the order the display logic steps through.
                "units": QUALIFIED_RE.findall(arguments[4]) if len(arguments) > 4 else [],
                # The unit used for mash thickness, where the system has one.
                "thickness": reference(arguments[5]) if len(arguments) > 5 else None,
            }
        )
    return systems


def parse_enum(text: str, name: str) -> list[str]:
    """Read the members of an `enum class`."""
    match = re.search(rf"enum class {name}\s*(?::[^{{]*)?(?={{)", text)
    if not match:
        raise Unsupported(f"no enum class {name}")
    start, end = match_balanced(text, match.end())
    members = []
    for entry in split_arguments(text[start + 1 : end - 1]):
        member = re.match(r"(\w+)", entry.strip())
        if member:
            members.append(member.group(1))
    return members


# ── Entry point ───────────────────────────────────────────────────────────────────────────────


def build() -> dict:
    unit_text = strip_comments(SRC_UNIT.read_text(encoding="utf-8"))
    system_text = strip_comments(SRC_UNIT_SYSTEM.read_text(encoding="utf-8"))
    quantity_text = strip_comments(SRC_PHYSICAL_QUANTITY.read_text(encoding="utf-8"))

    units = parse_units(unit_text)
    systems = parse_unit_systems(system_text)

    known_systems = {system["name"] for system in systems}
    for unit in units:
        if unit["unit_system"] not in known_systems:
            raise Unsupported(f"{unit['name']} refers to unknown unit system {unit['unit_system']}")

    return {
        "generated_by": "tools/extract-units.py",
        "source_files": [
            str(SRC_UNIT.relative_to(REPO)),
            str(SRC_UNIT_SYSTEM.relative_to(REPO)),
            str(SRC_PHYSICAL_QUANTITY.relative_to(REPO)),
        ],
        "physical_quantities": parse_enum(quantity_text, "PhysicalQuantity"),
        "systems_of_measurement": parse_enum(
            strip_comments((REPO / "src" / "measurement" / "SystemOfMeasurement.h").read_text(encoding="utf-8")),
            "SystemOfMeasurement",
        ),
        "unit_systems": systems,
        "units": units,
        "converted_by_formula": [unit["name"] for unit in units if unit["converts_by_formula"]],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    parser.add_argument("--summary", action="store_true", help="print a human-readable summary")
    args = parser.parse_args()

    document = build()
    if args.summary:
        for unit in document["units"]:
            how = "formula" if unit["converts_by_formula"] else f"x{unit['scale']:g}"
            canonical = unit["canonical"] or "(canonical)"
            print(f"{unit['name']:<28} {'/'.join(unit['abbreviations']):<10} {how:>18}  -> {canonical}")
        print(
            f"\n{len(document['units'])} units, {len(document['unit_systems'])} unit systems, "
            f"{len(document['physical_quantities'])} physical quantities, "
            f"{len(document['converted_by_formula'])} converted by formula"
        )
        return 0

    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
        print(f"wrote {args.out} ({len(document['units'])} units, {len(document['unit_systems'])} unit systems)")
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
