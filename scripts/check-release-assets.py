#!/usr/bin/env python3
"""Check that the release assets are names `gh extension install` can resolve.

`gh` computes `<GOOS>-<GOARCH>` for the running machine and picks the release
asset whose name ends in that string (plus `.exe` on Windows). A platform with
no matching asset is not a degraded install — it is a hard failure, on a user's
machine, long after CI went green.

ADR-014 calls that the catastrophic case, yet nothing verified it: CI only
proved that a locally built binary exists. This closes that gap by reading the
publish workflow and checking the names it will actually produce.

Usage:
    python3 scripts/check-release-assets.py [.github/workflows/publish-release.yaml]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Platform strings `gh` can compute. Anything outside this set can never be
# selected automatically, however sensible the name looks.
GH_PLATFORMS = {
    f"{os}-{arch}"
    for os in ("linux", "darwin", "windows", "freebsd", "android")
    for arch in ("amd64", "arm64", "386", "arm")
}

# Platforms we consider non-negotiable: leaving one out means those users
# cannot install at all.
REQUIRED = {
    "linux-amd64",
    "linux-arm64",
    "darwin-amd64",
    "darwin-arm64",
    "windows-amd64",
    "windows-arm64",
}

# Assets `gh` can never select, published for manual download only. Each needs a
# reason, so that an accidental typo cannot hide here.
MANUAL_ONLY = {
    "linux-amd64-musl": "for distributions without a compatible glibc",
}

BINARY = "gh-settings"


def main() -> int:
    path = Path(
        sys.argv[1] if len(sys.argv) > 1 else ".github/workflows/publish-release.yaml"
    )
    if not path.is_file():
        print(f"{path}: no such file", file=sys.stderr)
        return 1

    workflow = path.read_text()
    assets = re.findall(r"asset:\s*([A-Za-z0-9._-]+)", workflow)

    if not assets:
        print(f"{path}: no `asset:` entries found — has the matrix moved?", file=sys.stderr)
        return 1

    problems: list[str] = []

    duplicates = {name for name in assets if assets.count(name) > 1}
    if duplicates:
        problems.append(f"duplicate assets: {', '.join(sorted(duplicates))}")

    for name in assets:
        if name in GH_PLATFORMS or name in MANUAL_ONLY:
            continue
        problems.append(
            f"`{name}` is not a platform gh can compute, and is not listed as "
            f"manual-only. gh would never select it."
        )

    missing = REQUIRED - set(assets)
    for name in sorted(missing):
        problems.append(f"no asset for `{name}` — those users cannot install at all")

    # The staged filename must end in the platform string, or the suffix match
    # `gh` performs will not find it.
    if f'"dist/{BINARY}_${{{{ inputs.tag }}}}_${{{{ matrix.asset }}}}${{{{ matrix.ext }}}}"' not in workflow:
        problems.append(
            "the staged asset filename no longer ends in "
            "`<asset><ext>`; gh matches on that suffix"
        )

    if problems:
        print(f"{path}: release asset problems\n", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        print(
            "\nSee docs/adr/014-releases.md: a wrongly named asset makes the "
            "extension uninstallable.",
            file=sys.stderr,
        )
        return 1

    resolvable = sorted(name for name in assets if name in GH_PLATFORMS)
    print(f"{len(assets)} assets, {len(resolvable)} resolvable by gh:")
    for name in resolvable:
        print(f"  {BINARY}_<tag>_{name}")
    for name in sorted(set(assets) & MANUAL_ONLY.keys()):
        print(f"  {BINARY}_<tag>_{name}  (manual download: {MANUAL_ONLY[name]})")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
