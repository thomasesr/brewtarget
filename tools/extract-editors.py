#!/usr/bin/env python3
"""Extract the structure of Brewtarget's editor dialogs.

The Qt editors are laid out by hand in `ui/*Editor.ui` and bound to model properties in
`src/editors/*Editor.cpp`:

    EDITOR_FIELD_NORM(Hop, label_alpha, lineEdit_alpha, Hop::alpha_pct, 1)

Neither half is any use alone.  The `.ui` file knows the structure but names only
widgets; the `.cpp` knows which property a widget stands for but nothing about where it
belongs.  This joins them.

What is taken is the *architecture*: which editors exist, what tabs each has, how fields
are grouped, what order they come in, what each is called, what kind of control it wants
and to how many decimal places.  What is deliberately **not** taken is the layout --
grid coordinates, spacers, sizes, geometry.  Those are Qt's way of arranging a dialog in
1990s desktop style; the web interface arranges the same information its own way.  So a
field's row is read, because it says what comes before what, and then thrown away.

Usage:
    python3 tools/extract-editors.py --out schema/editors.json
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import xml.etree.ElementTree as ElementTree

from cpputil import match_balanced, scan, strip_comments

REPO = pathlib.Path(__file__).resolve().parent.parent

# `EDITOR_FIELD_NORM(Hop, label_alpha, lineEdit_alpha, Hop::alpha_pct, 1)`.  The trailing
# arguments differ by macro and are read per kind below.
FIELD_RE = re.compile(r"EDITOR_FIELD(_NORM|_ENUM|_COPQ|_PATH|_BEGIN|_END)?\s*(?=\()")

# What each Qt widget class amounts to as a control.  `Smart*` are Brewtarget's own line
# edits, which understand units; `Bt*` are its combo boxes and date pickers.  The web
# interface renders each of these its own way -- what matters is which it is, not that Qt
# called it a QLineEdit.
CONTROLS = {
    "SmartLineEdit": "measured",
    "QLineEdit": "text",
    "BtLineEditCurrency": "money",
    "QTextEdit": "long_text",
    "QPlainTextEdit": "long_text",
    "QCheckBox": "boolean",
    "BtComboBoxBool": "boolean",
    "BtComboBoxEnum": "choice",
    "QComboBox": "choice",
    "BtOptionalDateEdit": "date",
    "QDateEdit": "date",
    "QSpinBox": "whole_number",
    "QDoubleSpinBox": "number",
    "SmartValueDisplay": "calculated",
}


def split_arguments(body: str) -> list[str]:
    """Split a macro's arguments on the commas between them.

    Commas inside nested brackets belong to an argument, not between two, and `scan`
    skips string and character literals so a comma in one is left alone.
    """
    arguments, depth, start = [], 0, 0
    for index, char in scan(body):
        if char in "([{<":
            depth += 1
        elif char in ")]}>":
            depth -= 1
        elif char == "," and depth == 0:
            arguments.append(body[start:index].strip())
            start = index + 1
    arguments.append(body[start:].strip())
    return [argument for argument in arguments if argument]


def editor_fields(text: str) -> list[dict]:
    """Every `EDITOR_FIELD*` in one editor, in the order it is declared."""
    found = []
    for match in FIELD_RE.finditer(text):
        kind = (match.group(1) or "").removeprefix("_").lower() or "plain"
        if kind in {"begin", "end"}:
            continue
        begin, end = match_balanced(text, match.end(), "(", ")")
        arguments = split_arguments(text[begin + 1 : end - 1])
        if len(arguments) < 3:
            continue

        model, label, widget = arguments[0], arguments[1], arguments[2]
        if kind == "path":
            # Names no property: it is the folder the thing is filed in.
            property_name = "FolderPropertyBase::containedInFolderPath"
            rest = arguments[3:]
        else:
            if len(arguments) < 4:
                continue
            property_name = arguments[3].replace("PropertyNames::", "")
            # `EDITOR_FIELD_COPQ` puts the fields it controls where the precision would be.
            rest = arguments[5:] if kind == "copq" else arguments[4:]

        precision = int(rest[0]) if rest and re.fullmatch(r"\d+", rest[0]) else None
        found.append(
            {
                "model": model,
                "label_widget": label,
                "widget": widget,
                "property": property_name.strip(),
                "precision": precision,
                # Shown but never written back -- a computed inventory, say.
                "read_only": any("WhenToWriteField::Never" in argument for argument in rest),
            }
        )
    return found


def string_property(widget: ElementTree.Element, name: str) -> str | None:
    """A widget's string property, such as its text or its tool tip."""
    for prop in widget.findall("property"):
        if prop.get("name") == name:
            string = prop.find("string")
            if string is not None:
                return string.text or ""
    return None


def tab_titles(node: ElementTree.Element) -> list[str]:
    """The titles of a tab widget's pages, in order.

    The tree tabs carry an icon and no title, so their tool tip is what names them -- which is
    also what a person hovering one sees.
    """
    titles = []
    for page in node.findall("widget"):
        title = None
        for attribute in page.findall("attribute"):
            if attribute.get("name") in {"title", "toolTip"}:
                string = attribute.find("string")
                if string is not None and string.text:
                    title = title or string.text
        titles.append(title or page.get("name") or "")
    return titles


def walk(node: ElementTree.Element, page: str | None, group: str | None, order: list[int], into: dict) -> None:
    """Record which tab and group every widget belongs to, and what comes before what.

    `order` is a running counter: document order within a grid is not visual order, so a
    grid item's row is used where there is one, and the counter otherwise.
    """
    for child in node:
        if child.tag == "widget":
            name, cls = child.get("name"), child.get("class")
            # `BtTabWidget` is Brewtarget's own subclass, and the main window uses it for the
            # tabs that matter most.  Treating only `QTabWidget` as a tab widget misses them.
            if cls in {"QTabWidget", "BtTabWidget"}:
                for tab, title in zip(child.findall("widget"), tab_titles(child)):
                    walk(tab, title or None, None, order, into)
                continue
            if cls == "QGroupBox":
                walk(child, page, string_property(child, "title") or group, order, into)
                continue
            if name:
                order[0] += 1
                into.setdefault(name, {}).update(
                    {
                        "class": cls,
                        "page": page,
                        "group": group,
                        "text": string_property(child, "text"),
                        "tool_tip": string_property(child, "toolTip"),
                        "order": into.get(name, {}).get("order", order[0]),
                    }
                )
            walk(child, page, group, order, into)
        elif child.tag == "item":
            # A grid item says which row it is on, which is what puts the fields in order.
            row = child.get("row")
            if row is not None:
                for inner in child.iter("widget"):
                    if inner.get("name"):
                        into.setdefault(inner.get("name"), {})["order"] = int(row)
                        break
            walk(child, page, group, order, into)
        else:
            walk(child, page, group, order, into)


def layout_of(path: pathlib.Path) -> dict:
    """Every widget in one `.ui` file, by name."""
    root = ElementTree.parse(path).getroot()
    found: dict = {}
    for widget in root.findall("widget"):
        walk(widget, None, None, [1000], found)
    return found


# Which `.ui` file belongs to which editor, where the names do not simply correspond.
UI_FOR = {
    "StockPurchaseFermentableEditor": "stockPurchaseIngredientEditor",
    "StockPurchaseHopEditor": "stockPurchaseIngredientEditor",
    "StockPurchaseMiscEditor": "stockPurchaseIngredientEditor",
    "StockPurchaseYeastEditor": "stockPurchaseIngredientEditor",
    "StockUseIngredientEditor": "stockUseIngredientEditor",
}


def ui_path(editor: str) -> pathlib.Path | None:
    """The `.ui` file an editor is laid out in."""
    stem = UI_FOR.get(editor, editor[0].lower() + editor[1:])
    path = REPO / "ui" / f"{stem}.ui"
    return path if path.exists() else None


def build(editor: str, source: pathlib.Path) -> dict | None:
    """Join one editor's field bindings to the structure of its dialog."""
    fields = editor_fields(strip_comments(source.read_text(encoding="utf-8", errors="replace")))
    if not fields:
        return None
    ui = ui_path(editor)
    layout = layout_of(ui) if ui else {}

    built = []
    for field in fields:
        widget = layout.get(field["widget"], {})
        label = layout.get(field["label_widget"], {})
        built.append(
            {
                "property": field["property"],
                # The label as the dialog writes it.  A `SmartLabel` carries no text of its
                # own -- it takes it from the property at run time -- which is what
                # `schema/labels.json` is for, so this is left null and filled in there.
                "label": label.get("text") or None,
                "help": label.get("tool_tip") or widget.get("tool_tip") or None,
                "tab": widget.get("page"),
                "group": widget.get("group"),
                "control": CONTROLS.get(widget.get("class") or "", "text"),
                "precision": field["precision"],
                "read_only": field["read_only"],
                "order": widget.get("order", 10_000),
            }
        )

    # Into tabs, in the order the dialog has them, each field in the order it appears.
    tabs: list[dict] = []
    for field in sorted(built, key=lambda field: field["order"]):
        title = field.pop("tab") or "Main"
        tab = next((tab for tab in tabs if tab["title"] == title), None)
        if tab is None:
            tab = {"title": title, "fields": []}
            tabs.append(tab)
        field.pop("order")
        tab["fields"].append(field)
    # Which model the editor edits, so it can be joined to a table.  Every field in one
    # editor names the same one.
    models = {field["model"] for field in fields}
    return {
        "editor": editor,
        "cpp_type": sorted(models)[0] if models else None,
        "ui": str(ui.relative_to(REPO)) if ui else None,
        "tabs": tabs,
    }


# The main window binds its fields the same way the dialogs do, under another name:
# `SMART_FIELD_INIT(MainWindow, label_boilTime, value_boilTime, Boil, PropertyNames::Boil::boilTime_mins, 0)`.
SMART_FIELD_RE = re.compile(r"SMART_FIELD_INIT(_NO_SF|_FIXED)?\s*(?=\()")


def main_window_fields(text: str) -> dict[str, dict]:
    """Every field the main window binds, by the widget that shows it."""
    found = {}
    for match in SMART_FIELD_RE.finditer(text):
        begin, end = match_balanced(text, match.end(), "(", ")")
        arguments = split_arguments(text[begin + 1 : end - 1])
        if match.group(1) == "_NO_SF":
            # No separate field: the label is the display.  These are the figures the recipe
            # is reckoned to come to -- OG, FG, colour -- which are shown and never typed.
            if len(arguments) < 4:
                continue
            _owner, label, model, property_name = arguments[:4]
            widget = label
            rest = arguments[4:]
        else:
            if len(arguments) < 5:
                continue
            _owner, label, widget, model, property_name = arguments[:5]
            rest = arguments[5:]
        precision = int(rest[0]) if rest and re.fullmatch(r"\d+", rest[0]) else None
        found[widget] = {
            "label_widget": label,
            "model": model,
            "property": property_name.replace("PropertyNames::", "").strip(),
            "precision": precision,
            "shown_only": match.group(1) == "_NO_SF",
        }
    return found


# The parts of the application that bind fields the way the dialogs do, and the `.ui` file
# each is laid out in.  Every one of these is a tab or a panel rather than a window of its own.
PANELS = {
    "main_window": ("src/MainWindow.cpp", "ui/mainWindow.ui", "Recipe"),
    "recipe_extras": ("src/widgets/RecipeExtrasWidget.cpp", "ui/recipeExtrasWidget.ui", None),
    "recipe_water": ("src/widgets/RecipeWaterWidget.cpp", "ui/recipeWaterWidget.ui", None),
}


def panel(source_name: str, ui_name: str, only_page: str | None) -> dict:
    """The fields one panel binds, in the order its layout has them.

    `only_page` narrows it to a single tab, for a file that lays out more than one.
    """
    source = REPO / source_name
    bindings = main_window_fields(strip_comments(source.read_text(encoding="utf-8", errors="replace")))

    path = REPO / ui_name
    root = ElementTree.parse(path).getroot()
    layout: dict = {}
    for widget in root.findall("widget"):
        walk(widget, None, None, [1000], layout)

    strips = {}
    for node in root.iter("widget"):
        if node.get("class") in {"QTabWidget", "BtTabWidget"}:
            strips[node.get("name") or ""] = tab_titles(node)

    fields = []
    for widget, binding in bindings.items():
        where = layout.get(widget, {})
        if only_page is not None and where.get("page") != only_page:
            continue
        label = layout.get(binding["label_widget"], {})
        fields.append(
            {
                "property": binding["property"],
                "model": binding["model"],
                # `&Name` is Qt's way of marking a keyboard shortcut; the ampersand is not
                # part of what is shown.
                "label": (label.get("text") or "").replace("&", "") or None,
                "group": where.get("group"),
                "control": CONTROLS.get(where.get("class") or "", "text"),
                "precision": binding["precision"],
                "shown_only": binding["shown_only"],
                "order": where.get("order", 10_000),
            }
        )
    fields.sort(key=lambda field: field["order"])
    for field in fields:
        field.pop("order")

    return {
        "ui": ui_name,
        "trees": strips.get("tabWidget_Trees", []),
        "recipe_tabs": strips.get("tabWidget_recipeView", []),
        "ingredient_tabs": strips.get("tabWidget_ingredients", []),
        "fields": fields,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=pathlib.Path, help="write JSON here instead of stdout")
    args = parser.parse_args()

    editors = {}
    sources = []
    for source in sorted((REPO / "src" / "editors").glob("*Editor.cpp")):
        editor = source.stem
        built = build(editor, source)
        if built is None:
            continue
        editors[editor] = built
        sources.append(str(source.relative_to(REPO)))

    panels = {}
    for name, (source_name, ui_name, only_page) in PANELS.items():
        panels[name] = panel(source_name, ui_name, only_page)
        sources.append(source_name)

    document = {
        "generated_by": "tools/extract-editors.py",
        "source_files": sorted(sources),
        # How the application itself is arranged, as distinct from its dialogs: the main
        # window and the panels its recipe tabs are made of.
        "panels": panels,
        "editors": dict(sorted(editors.items())),
    }
    text = json.dumps(document, indent=2) + "\n"
    if args.out:
        args.out.write_text(text, encoding="utf-8")
        fields = sum(len(tab["fields"]) for editor in editors.values() for tab in editor["tabs"])
        panel_fields = sum(len(found["fields"]) for found in panels.values())
        print(
            f"wrote {args.out} ({len(editors)} editors, {fields} fields; "
            f"{len(panels)} panels, {panel_fields} fields)",
            file=sys.stderr,
        )
    else:
        print(text, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
