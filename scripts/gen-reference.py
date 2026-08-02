#!/usr/bin/env python3
"""Generate the configuration reference from the JSON Schema.

The schema is the public contract (ADR-007) and is itself generated from the
Rust types, so deriving the documentation from it means the reference cannot
disagree with what the tool actually accepts.

Usage:
    gh-settings schema | python3 scripts/gen-reference.py > docs/configuration.md
"""

from __future__ import annotations

import json
import sys
from typing import Any

HEADER = """# Configuration reference

<!--
  Generated from the JSON Schema by scripts/gen-reference.py.
  Do not edit by hand: edit the Rust types and run `mise run docs:reference`.
-->

Every section is optional. **An absent section is unmanaged**: nothing is read,
diffed or written for it. That is what makes adoption incremental — you can
manage labels alone and nothing else will move.

Add this line to get completion and validation in your editor:

```yaml
# yaml-language-server: $schema=https://noirbizarre.github.io/gh-settings/schema/v1/settings.json
```
"""


def resolve(schema: dict[str, Any], root: dict[str, Any]) -> dict[str, Any]:
    """Follow a local `$ref`, merging any sibling keywords."""
    ref = schema.get("$ref")
    if not ref or not ref.startswith("#/"):
        return schema

    target: Any = root
    for part in ref.removeprefix("#/").split("/"):
        target = target.get(part, {})

    merged = dict(target)
    merged.update({k: v for k, v in schema.items() if k != "$ref"})
    return merged


def unwrap(schema: dict[str, Any], root: dict[str, Any]) -> dict[str, Any]:
    """Peel away the wrappers serde/schemars adds around optional values.

    `Option<T>` becomes `anyOf: [T, null]`, which hides `T`'s properties one
    level down. Untagged enums such as `Prunable<T>` become `anyOf` too; for
    documentation purposes the object branch is the interesting one, since the
    list branch is just its `items` field.
    """
    schema = resolve(schema, root)

    for keyword in ("anyOf", "oneOf"):
        options = schema.get(keyword)
        if not options:
            continue

        branches = [
            resolve(option, root)
            for option in options
            if resolve(option, root).get("type") != "null"
        ]
        if not branches:
            continue

        # Prefer a branch that actually documents fields.
        documented = [b for b in branches if b.get("properties")]
        chosen = documented[0] if documented else branches[0]

        merged = dict(chosen)
        # The outer description is the one written for the user ("Issue and pull
        # request labels"); the branch's is an internal note about the shape.
        if description := schema.get("description"):
            merged["description"] = description
        return unwrap(merged, root) if merged is not chosen else merged

    return schema


def type_of(schema: dict[str, Any], root: dict[str, Any]) -> str:
    """Render a human-readable type for a property."""
    schema = resolve(schema, root)

    if enum := schema.get("enum"):
        return " \\| ".join(f"`{value}`" for value in enum)

    # `Option<T>` shows up as `anyOf: [T, null]`; the null is noise here.
    for keyword in ("anyOf", "oneOf"):
        if options := schema.get(keyword):
            rendered = [
                type_of(option, root)
                for option in options
                if resolve(option, root).get("type") != "null"
            ]
            unique = list(dict.fromkeys(rendered))
            return " \\| ".join(unique) if unique else "any"

    declared = schema.get("type")
    if isinstance(declared, list):
        declared = next((t for t in declared if t != "null"), "any")

    if declared == "array":
        return f"list of {type_of(schema.get('items', {}), root)}"
    if declared == "object":
        return "object"
    if declared == "integer":
        return "integer"
    if declared == "boolean":
        return "boolean"
    if declared == "string":
        return "string"
    return "any"


def describe(schema: dict[str, Any], root: dict[str, Any]) -> str:
    """First paragraph of a property's documentation."""
    schema = unwrap(schema, root)
    text = schema.get("description", "").strip()
    return text.split("\n\n")[0].replace("\n", " ") if text else ""


def render_object(
    name: str, schema: dict[str, Any], root: dict[str, Any], level: int
) -> list[str]:
    """Render one section as a heading plus a property table."""
    schema = unwrap(schema, root)
    lines = [f"\n{'#' * level} `{name}`\n"]

    if description := schema.get("description", "").strip():
        lines.append(f"{description}\n")

    properties = schema.get("properties")
    if not properties:
        lines.append(f"Type: {type_of(schema, root)}\n")
        return lines

    required = set(schema.get("required", []))
    lines.append("| Field | Type | Required | Description |")
    lines.append("|---|---|---|---|")

    for key, value in properties.items():
        lines.append(
            f"| `{key}` | {type_of(value, root)} | "
            f"{'yes' if key in required else 'no'} | {describe(value, root)} |"
        )

    lines.append("")
    return lines


def main() -> int:
    schema = json.load(sys.stdin)
    root = schema

    out = [HEADER]

    for name, section in schema.get("properties", {}).items():
        out.extend(render_object(name, section, root, level=2))

        # Document nested object types one level deep; deeper nesting is better
        # served by the schema itself than by an unreadable table.
        resolved = unwrap(section, root)
        for key, value in (resolved.get("properties") or {}).items():
            nested = unwrap(value, root)
            if nested.get("properties"):
                out.extend(render_object(f"{name}.{key}", nested, root, level=3))
            # A list of objects: document the element type.
            items = unwrap(nested.get("items", {}), root) if nested.get("items") else {}
            if items.get("properties"):
                out.extend(render_object(f"{name}.{key}[]", items, root, level=3))

    print("\n".join(out))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
