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
# $schema: https://noirbizarre.github.io/gh-settings/schema/v1/settings.json
```
"""

# Deepest heading level the walk will emit, enough for
# `environments.items[].variables[]`.
MAX_LEVEL = 5


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


def enum_values(schema: dict[str, Any], root: dict[str, Any]) -> list[Any]:
    """Allowed values, however schemars chose to spell them.

    A Rust enum comes out as `oneOf: [{const: "active"}, …]`, not as `enum`.
    Reading only the `enum` keyword meant every such field was documented as a
    bare `string` with no hint of what it accepts — on the page that is supposed
    to be the reference for exactly that.
    """
    schema = resolve(schema, root)

    if "const" in schema:
        return [schema["const"]]
    if enum := schema.get("enum"):
        return list(enum)

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
            return []

        values: list[Any] = []
        for branch in branches:
            found = enum_values(branch, root)
            # One branch that is not a fixed value makes the whole thing a
            # union rather than an enumeration, and listing part of it would
            # imply the rest is not allowed.
            if not found:
                return []
            values.extend(found)
        return values

    return []


def accepts_bare_list(schema: dict[str, Any], root: dict[str, Any]) -> bool:
    """Whether a section may also be written as a plain list.

    `Prunable<T>` is an untagged enum: either a bare list, or an object with
    `items` and `prune`. `unwrap` keeps the object branch because it is the one
    with fields to document, which left the bare list — the form the README and
    the CI configuration both use — documented nowhere.
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
        has_list = any(branch.get("type") == "array" for branch in branches)
        has_object = any(branch.get("properties") for branch in branches)
        if has_list and has_object:
            return True

        # `Option<Prunable<T>>` is an `anyOf` wrapping an `anyOf`. The optional
        # wrapper is not the interesting one, so look through it.
        if len(branches) == 1:
            return accepts_bare_list(branches[0], root)

    return False


def type_of(schema: dict[str, Any], root: dict[str, Any]) -> str:
    """Render a human-readable type for a property."""
    schema = resolve(schema, root)

    if values := enum_values(schema, root):
        return " \\| ".join(f"`{value}`" for value in values)

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
    """First paragraph of a property's documentation, plus its default.

    The default is appended rather than given a column of its own: only a
    handful of fields have one, and a mostly-empty column costs every row.
    """
    # Read the default before unwrapping: schemars puts it on the property, next
    # to the `$ref`, and unwrapping follows the ref away from it.
    resolved = resolve(schema, root)
    default = resolved.get("default", schema.get("default"))

    text = unwrap(schema, root).get("description", "").strip()
    text = text.split("\n\n")[0].replace("\n", " ") if text else ""

    if default is not None:
        rendered = json.dumps(default) if not isinstance(default, str) else default
        suffix = f"Defaults to `{rendered}`."
        text = f"{text} {suffix}".strip() if text else suffix

    return text


def render_object(
    name: str, schema: dict[str, Any], root: dict[str, Any], level: int
) -> list[str]:
    """Render one section as a heading plus a property table."""
    bare_list = accepts_bare_list(schema, root)
    schema = unwrap(schema, root)
    lines = [f"\n{'#' * level} `{name}`\n"]

    if description := schema.get("description", "").strip():
        lines.append(f"{description}\n")

    if bare_list:
        lines.append(
            "May also be written as a bare list, which is the same as giving "
            "`items` with `prune: false`.\n"
        )

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


def walk(
    name: str,
    schema: dict[str, Any],
    root: dict[str, Any],
    level: int,
    seen: set[str],
) -> list[str]:
    """Render a section and every object type nested inside it."""
    out = render_object(name, schema, root, level)

    # Markdown headings below h5 stop being navigable, and the schema itself
    # serves deeper nesting better than an unreadable table would. Nothing in
    # the configuration reaches this depth today; the cap is here so that adding
    # something that does fails visibly rather than silently truncating.
    if level >= MAX_LEVEL:
        return out

    resolved = unwrap(schema, root)
    for key, value in (resolved.get("properties") or {}).items():
        nested = unwrap(value, root)
        # The `seen` set keeps a self-referential type from recursing forever.
        path = f"{name}.{key}"
        if nested.get("properties") and path not in seen:
            seen.add(path)
            out.extend(walk(path, nested, root, level + 1, seen))

        # A list of objects: document the element type.
        items = unwrap(nested.get("items", {}), root) if nested.get("items") else {}
        path = f"{name}.{key}[]"
        if items.get("properties") and path not in seen:
            seen.add(path)
            out.extend(walk(path, items, root, level + 1, seen))

    return out


def main() -> int:
    schema = json.load(sys.stdin)
    root = schema

    out = [HEADER]

    for name, section in schema.get("properties", {}).items():
        out.extend(walk(name, section, root, level=2, seen=set()))

    # Exactly one trailing newline. Emitting two would put this generator in a
    # fight with prek's end-of-file-fixer, and the "is the committed copy
    # current?" check would never converge.
    sys.stdout.write("\n".join(out).rstrip("\n") + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
