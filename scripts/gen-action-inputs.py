#!/usr/bin/env python3
"""Generate the action's input table from `action.yml`.

Same principle as the schema, the CLI reference and the permission table: a
document describing the code is produced *by* the code, so it cannot drift.
An input added to `action.yml` without touching the docs fails CI.

Usage:
    python3 scripts/gen-action-inputs.py | python3 scripts/splice.py docs/actions.md
"""

from __future__ import annotations

import sys
from pathlib import Path

import yaml

BEGIN = "<!-- generated: do not edit below -->"
END = "<!-- /generated -->"


def one_line(text: str) -> str:
    """Collapse a multi-line description for a table cell."""
    return " ".join(text.split()).replace("|", "\\|")


def main() -> int:
    action = yaml.safe_load(Path("action.yml").read_text())
    inputs = action.get("inputs") or {}

    out = [BEGIN, "", "| Input | Default | Description |", "|---|---|---|"]

    for name, spec in inputs.items():
        default = spec.get("default", "")
        # `${{ github.token }}` would be interpreted if a workflow ever embedded
        # this table, and reads better as code regardless.
        rendered = f"`{default}`" if default not in ("", None) else ""
        out.append(f"| `{name}` | {rendered} | {one_line(spec.get('description', ''))} |")

    out += ["", END]
    sys.stdout.write("\n".join(out) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
