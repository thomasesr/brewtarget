#!/usr/bin/env python3
"""Extract the names Brewtarget shows people for its fields and its enumerated values.

The database column `fine_grind_yield_pct` is shown as "Yield (DBFG) %", and the stored
string `drain mash tun` is shown as "Drain Mash Tun".  Neither of those is derivable from
what is stored, and guessing produces labels that are merely plausible -- which is worse
than wrong, because nobody notices.

Two declarations carry them, both in `src/model/`:

  QString Fermentable::localisedName_fineGrindYield_pct() { return tr("Yield (DBFG) %"); }

  EnumDisplayNameMapping const Fermentable::typeDisplayNames {"Fermentable", {
     {Fermentable::Type::Dry_Extract, QT_TRANSLATE_NOOP("Fermentable", "Dry Extract")},
  }};

Both are wrapped in Qt's translation machinery, so this is also the list of strings a
translation would have to cover -- see PLAN.md §7, which has labels and translations as
one decision because they come from one place.

Usage:
    python3 tools/extract-labels.py --out schema/labels.json
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

from cpputil import match_balanced, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent
SOURCE_DIRS = ["src/model", "src/measurement"]

# `QString Fermentable::localisedName_colour() { return tr("Colour"); }` -- the owner is
# written out because the function is defined outside the class.
DEFINED_LABEL_RE = re.compile(
    r"QString\s+(\w+)::localisedName_(\w+)\s*\(\s*\)\s*\{\s*return\s+tr\(\s*\"((?:[^\"\\]|\\.)*)\"\s*\)",
)
# `static QString localisedName_folderPath() { return Derived::tr("Folder Path"); }` -- declared
# inside a class template, so the owner is the template rather than any one model class.  These
# are the shared fields every entity has, and the owner is taken from the file name.
INLINE_LABEL_RE = re.compile(
    r"static\s+QString\s+localisedName_(\w+)\s*\(\s*\)\s*\{\s*return\s+(?:\w+::)?tr\(\s*\"((?:[^\"\\]|\\.)*)\"\s*\)",
)
DISPLAY_MAPPING_RE = re.compile(r"EnumDisplayNameMapping const (?:([\w:]+?)::)?(\w+)\s*(?=\{)")
# `{Fermentable::Type::Dry_Extract, QT_TRANSLATE_NOOP("Fermentable", "Dry Extract")}`
DISPLAY_ENTRY_RE = re.compile(
    r"\{\s*([\w:]+)\s*,\s*QT_TRANSLATE_NOOP\(\s*\"[^\"]*\"\s*,\s*\"((?:[^\"\\]|\\.)*)\"\s*\)\s*\}",
)


def unescape(text: str) -> str:
    """Resolve the escapes a C++ string literal may contain."""
    return text.encode().decode("unicode_escape")


def labels_in(text: str, file_name: str) -> dict[str, str]:
    """Every `localisedName_*` in one file, keyed by the property it names."""
    found: dict[str, str] = {}
    for owner, member, label in DEFINED_LABEL_RE.findall(text):
        found[f"{owner}::{member}"] = unescape(label)
    # A label declared inside a class template belongs to the template, which is named after
    # the file it is in -- `StepBase.h` declares `StepBase`'s.
    owner = pathlib.Path(file_name).stem
    for member, label in INLINE_LABEL_RE.findall(text):
        found.setdefault(f"{owner}::{member}", unescape(label))
    return found


def display_names_in(text: str) -> dict[str, dict[str, str]]:
    """Every `EnumDisplayNameMapping`, keyed as the enum mappings are keyed."""
    found: dict[str, dict[str, str]] = {}
    for match in DISPLAY_MAPPING_RE.finditer(text):
        owner, name = match.group(1), match.group(2)
        try:
            # The regex ends on a lookahead, so the opening brace is at `match.end()`.
            begin, end = match_balanced(text, match.end())
        except ValueError:
            # A declaration whose body is elsewhere, or nowhere.
            continue
        body = text[begin:end]
        entries = {member.rsplit("::", 1)[-1]: unescape(label) for member, label in DISPLAY_ENTRY_RE.findall(body)}
        if entries:
            found[f"{owner}::{name}" if owner else name] = entries
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    args = parser.parse_args()

    labels: dict[str, str] = {}
    display_names: dict[str, dict[str, str]] = {}
    sources: list[str] = []

    for directory in SOURCE_DIRS:
        for path in sorted((REPO / directory).rglob("*")):
            if path.suffix not in {".cpp", ".h"}:
                continue
            text = strip_comments(path.read_text(encoding="utf-8", errors="replace"))
            found_labels = labels_in(text, path.name)
            found_names = display_names_in(text)
            if found_labels or found_names:
                sources.append(str(path.relative_to(REPO)))
            labels.update(found_labels)
            display_names.update(found_names)

    document = {
        "generated_by": "tools/extract-labels.py",
        "source_files": sources,
        # Keyed by property, as `schema/v23.json` keys its columns.
        "labels": dict(sorted(labels.items())),
        # Keyed by mapping name and then by enum member, as `schema/enums.json` is.
        "display_names": {name: dict(sorted(values.items())) for name, values in sorted(display_names.items())},
    }
    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.write_text(text, encoding="utf-8")
        print(f"wrote {args.out} ({len(labels)} labels, {len(display_names)} enumerations)", file=sys.stderr)
    else:
        print(text, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
