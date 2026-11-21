#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-calc-data.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract the reference data the brewing calculations depend on.

Brewing formulae are full of empirical constants and lookup tables, and a transcription slip in
one of them shows up as a recipe that is quietly a little wrong rather than as a crash.  So lift
them: the physical constants, the HMRC table that turns a gravity drop into alcohol by volume, and
the atomic weights the water chemistry works from.

    python3 tools/extract-calc-data.py --out schema/calculations.json

See PLAN.md Phase 5.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

from cpputil import match_balanced, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
SRC_ALGORITHMS = REPO / "src" / "calculations" / "Algorithms.cpp"
SRC_CONSTANTS = REPO / "src" / "measurement" / "PhysicalConstants.h"

CONSTANT_RE = re.compile(r"double\s+constexpr\s+(\w+)\s*=\s*([^;]+);")
ROW_RE = re.compile(r"\{([^{}]*)\}")


def arithmetic(expression: str, known: dict[str, float]) -> float | None:
    """Evaluate a constant expression, which may refer to constants already read."""
    expression = expression.strip()
    if not re.fullmatch(r"[\w\s./*+()-]+", expression):
        return None
    try:
        return float(eval(expression, {"__builtins__": {}}, dict(known)))  # noqa: S307
    except (NameError, SyntaxError, ZeroDivisionError, TypeError):
        return None


def parse_constants(text: str) -> dict[str, float]:
    """Read `double constexpr name = ...;`, resolving references to earlier constants."""
    constants: dict[str, float] = {}
    for name, expression in CONSTANT_RE.findall(text):
        value = arithmetic(expression, constants)
        if value is not None:
            constants[name] = value
    return constants


def parse_table(text: str, declaration: str, fields: list[str]) -> list[dict]:
    match = re.search(re.escape(declaration) + r"\s*(?=\{)", text)
    if not match:
        raise ValueError(f"could not find {declaration}")
    start, end = match_balanced(text, match.end())
    rows = []
    for row in ROW_RE.findall(text[start + 1 : end - 1]):
        values = [float(value) for value in row.split(",") if value.strip()]
        if len(values) != len(fields):
            raise ValueError(f"{declaration}: row {row!r} has {len(values)} values")
        rows.append(dict(zip(fields, values)))
    return rows


def build() -> dict:
    algorithms = strip_comments(SRC_ALGORITHMS.read_text(encoding="utf-8"))
    constants = strip_comments(SRC_CONSTANTS.read_text(encoding="utf-8"))
    return {
        "generated_by": "tools/extract-calc-data.py",
        "source_files": [str(SRC_ALGORITHMS.relative_to(REPO)), str(SRC_CONSTANTS.relative_to(REPO))],
        "physical_constants": parse_constants(constants),
        # HM Revenue and Customs' table for working out alcohol by volume from the drop in
        # gravity, which is what UK microbreweries without laboratory facilities are told to use.
        "gravity_difference_factors": parse_table(
            algorithms,
            "QVector<AbvFactorForGravityDifference> const gravityDifferenceFactors",
            ["excess_gravity_diff_x10_min", "excess_gravity_diff_x10_max", "pct_abv_min", "pct_abv_max", "factor"],
        ),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    args = parser.parse_args()

    document = build()
    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
        print(
            f"wrote {args.out} ({len(document['physical_constants'])} constants, "
            f"{len(document['gravity_difference_factors'])} gravity factor rows)"
        )
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
