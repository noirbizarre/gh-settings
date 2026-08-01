# 8. YAML spans come from a side index for precise diagnostics

## Status

Accepted.

## Context

Good diagnostics underline the exact offending value. That needs byte spans.

`serde_yaml` was archived in 2024. Its maintained forks deserialize well but
expose only a coarse error location, not a span per field. Parsers that do
expose spans (`saphyr`, `marked-yaml`) do not integrate with serde.

## Decision

Parse twice, deliberately:

1. `saphyr` produces a tree carrying byte spans, from which a
   `path -> (key span, value span)` index is built;
2. `serde_yaml_ng`, wrapped in `serde_path_to_error`, deserializes into the
   typed `Settings` and reports the *path* of any failure.

Looking a reported path up in the index turns `invalid type: string` into an
underline beneath the offending value. Errors about a field's existence — unknown
or missing — underline the key instead of the value.

Validation findings produced by resources use the same index, so hand-written
checks and serde failures render identically.

## Consequences

* Diagnostics point at exactly the right characters.
* Parsing happens twice. On files of this size the cost is irrelevant.
* A `saphyr` parse failure degrades to an empty index rather than an error: the
  serde pass produces the user-facing syntax error, and it does so better.
