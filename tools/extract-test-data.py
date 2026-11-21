#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
#
# tools/extract-test-data.py is part of Brewtarget, and is copyright the following authors 2026:
#   • The Brewtarget contributors
#
# Brewtarget is free software: you can redistribute it and/or modify it under the terms of the GNU General Public
# License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later
# version.  See <http://www.gnu.org/licenses/>.
"""Extract expected values from the C++ unit tests, so the Rust tests can assert the same ones.

`src/unitTests/` is the behavioural oracle for this port: a ported module is only correct if it
agrees with the C++ tests, to the tolerance those tests use.  Where a test carries a table of
expected values, lifting it is better than retyping it -- and means a change upstream shows up as
a diff rather than going unnoticed.

    python3 tools/extract-test-data.py --out testData/expected

See PLAN.md Phase 3.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

from cpputil import match_balanced, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
SRC_MEASUREMENT_TESTS = REPO / "src" / "unitTests" / "MeasurementTests.cpp"

ROW_RE = re.compile(r"\{([^{}]*)\}")


def table(text: str, declaration: str, fields: list[str]) -> list[dict]:
    """Read a braced table of numeric literals into a list of records."""
    match = re.search(re.escape(declaration) + r"\s*(?=\{)", text)
    if not match:
        raise ValueError(f"could not find {declaration}")
    start, end = match_balanced(text, match.end())
    rows = []
    for row in ROW_RE.findall(text[start + 1 : end - 1]):
        values = [float(value) for value in row.split(",") if value.strip()]
        if len(values) != len(fields):
            raise ValueError(f"{declaration}: row {row!r} has {len(values)} values, expected {len(fields)}")
        rows.append(dict(zip(fields, values)))
    return rows


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, required=True, help="directory to write the JSON files into")
    args = parser.parse_args()

    measurement = strip_comments(SRC_MEASUREMENT_TESTS.read_text(encoding="utf-8"))
    documents = {
        "sg-brix-equivalence.json": {
            "generated_by": "tools/extract-test-data.py",
            "source_file": str(SRC_MEASUREMENT_TESTS.relative_to(REPO)),
            "description": (
                "Specific gravity and degrees Brix, with the error margin each direction of the "
                "conversion is allowed, from MeasurementTests::testAlgorithms."
            ),
            "rows": table(
                measurement,
                "QVector<SgBrixEquivalance> const sgBrixEquivalances",
                ["sg", "brix", "error_margin_sg_to_brix", "error_margin_brix_to_sg"],
            ),
        }
    }

    args.out.mkdir(parents=True, exist_ok=True)
    for name, document in documents.items():
        (args.out / name).write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {args.out / name} ({len(document['rows'])} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
