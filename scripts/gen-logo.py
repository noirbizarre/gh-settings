#!/usr/bin/env python3
"""The two fixups that turn `logo.src.svg` into a publishable `logo.svg`.

Both sit either side of `usvg`, which does the real work of converting the
wordmark to paths — see the `logo` task in `mise.toml`:

    python3 scripts/gen-logo.py inline docs/images/logo.src.svg \\
      | usvg --font-family "Fira Sans" - -c \\
      | python3 scripts/gen-logo.py viewbox > docs/images/logo.svg

`inline` resolves the mark reference. `logo.src.svg` points at the icon with

    <image href="icon.svg" x="0" y="0" width="700" height="700"/>

so that the source stays a valid, previewable SVG and the mark has a single
definition. That reference cannot survive into the published logo, though:
`usvg` turns it into a nested `image/svg+xml` data URI, and GitHub's content
security policy blocks nested resources inside an SVG — the mark would simply
not appear in the README. So it is resolved here instead, into a plain
`<g transform=...>` around the icon's own paths. Nothing is copied by hand, so
the logo cannot drift from the icon.

`viewbox` puts back the `viewBox` that `usvg` drops. Without one an SVG has no
intrinsic aspect ratio, so `<img src="logo.svg" width="520">` sets the viewport
to 520px and leaves the content at full size — cropping it instead of scaling
it. Every consumer sizes the logo by width, so this is not cosmetic.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# The `<image>` element standing in for the mark. Attribute order is fixed
# rather than parsed loosely: this file is ours, and a surprise here should be
# a loud failure, not a silently mangled logo.
PLACEHOLDER = re.compile(
    r'<image\s+href="(?P<href>[^"]+)"\s+'
    r'x="(?P<x>[-\d.]+)"\s+y="(?P<y>[-\d.]+)"\s+'
    r'width="(?P<width>[\d.]+)"\s+height="(?P<height>[\d.]+)"\s*/>'
)

ROOT_SVG = re.compile(r"<svg\b[^>]*>(?P<body>.*)</svg>", re.DOTALL)
VIEWBOX = re.compile(r'viewBox="\s*([-\d.]+)[,\s]+([-\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)\s*"')
SIZED_ROOT = re.compile(r'(<svg\b[^>]*?)width="(?P<w>[\d.]+)"\s+height="(?P<h>[\d.]+)"')


def inline(source: Path) -> str:
    text = source.read_text()

    match = PLACEHOLDER.search(text)
    if match is None:
        raise SystemExit(
            f"{source}: no <image href=... x=... y=... width=... height=.../> placeholder found"
        )

    icon_path = source.parent / match["href"]
    if not icon_path.is_file():
        raise SystemExit(f"{source}: referenced {match['href']} does not exist")

    icon = icon_path.read_text()

    box = VIEWBOX.search(icon)
    if box is None:
        raise SystemExit(f"{icon_path}: no viewBox, so it cannot be scaled into the logo")
    min_x, min_y, box_w, box_h = (float(value) for value in box.groups())

    body = ROOT_SVG.search(icon)
    if body is None:
        raise SystemExit(f"{icon_path}: no <svg> root element")

    # Uniform scale, chosen from whichever axis is the tighter fit, so the mark
    # keeps its aspect ratio — the same contract as the default
    # `preserveAspectRatio="xMidYMid meet"` the placeholder would have had.
    scale = min(float(match["width"]) / box_w, float(match["height"]) / box_h)
    dx = float(match["x"]) - min_x * scale
    dy = float(match["y"]) - min_y * scale

    group = (
        f'<g transform="translate({dx:g} {dy:g}) scale({scale:g})">'
        f"{body['body'].strip()}"
        "</g>"
    )

    return text[: match.start()] + group + text[match.end() :]


def viewbox(text: str) -> str:
    if VIEWBOX.search(text):
        return text

    match = SIZED_ROOT.search(text)
    if match is None:
        raise SystemExit("no <svg> root with width and height, so no viewBox can be derived")

    # usvg normalises the origin, so the box always starts at 0 0.
    inserted = f'{match[1]}viewBox="0 0 {match["w"]} {match["h"]}" width="{match["w"]}" height="{match["h"]}"'
    return text[: match.start()] + inserted + text[match.end() :]


def main() -> int:
    match sys.argv[1:]:
        case ["inline", source]:
            sys.stdout.write(inline(Path(source)))
        case ["viewbox"]:
            sys.stdout.write(viewbox(sys.stdin.read()))
        case _:
            print(
                f"usage: {sys.argv[0]} inline <logo.src.svg>\n"
                f"       {sys.argv[0]} viewbox  < svg > svg",
                file=sys.stderr,
            )
            return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
