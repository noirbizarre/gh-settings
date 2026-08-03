#!/usr/bin/env python3
"""Splice a generated block into the marked region of a Markdown file.

Some documentation pages are entirely generated; others — `authentication.md`
most of all — are mostly hand-written prose with one generated table in the
middle. Regenerating those wholesale would destroy the prose, so the generator
writes only between the markers:

    <!-- generated: do not edit below -->
    ...replaced...
    <!-- /generated -->

Usage:
    gh-settings internal requirements | python3 scripts/splice.py docs/authentication.md
"""

from __future__ import annotations

import sys
from pathlib import Path

BEGIN = "<!-- generated: do not edit below -->"
END = "<!-- /generated -->"


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <file>", file=sys.stderr)
        return 2

    target = Path(sys.argv[1])
    generated = sys.stdin.read().strip("\n")

    if not target.is_file():
        print(f"{target}: no such file", file=sys.stderr)
        return 1

    original = target.read_text()

    start = original.find(BEGIN)
    end = original.find(END)

    if start == -1 or end == -1:
        print(
            f"{target}: no generated region found.\n"
            f"Add the markers around the block to replace:\n  {BEGIN}\n  {END}",
            file=sys.stderr,
        )
        return 1

    if end < start:
        print(f"{target}: the end marker precedes the begin marker", file=sys.stderr)
        return 1

    # `generated` carries its own markers, so replace the whole span including
    # the existing ones rather than only what sits between them.
    updated = original[:start] + generated + original[end + len(END) :]

    if updated != original:
        target.write_text(updated)
        print(f"updated {target}", file=sys.stderr)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
