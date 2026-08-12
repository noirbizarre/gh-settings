# Agent notes

Conventions that are easy to violate without noticing.

## Non-negotiable properties

* **An omitted configuration field is unmanaged.** Never reset a field to a
  default because the user did not mention it. A file setting only `homepage`
  must not touch the description.
* **Pruning is opt-in.** Never delete anything unless `prune: true` or `--prune`
  says so.
* **`diff` is pure.** Synchronous, total, no I/O. Network work belongs in
  `prepare()`.
* **`plan` never writes.** The transport is constructed read-only for it, and a
  write attempt panics deliberately.
* **Normalise both sides.** Anything not normalised becomes a permanent diff.
* **Never claim certainty about permissions.** `doctor` reporting "unknown" is a
  correct outcome; guessing is not.

## Layout

```
src/config/     parse, span index, diagnostics, Settings (the public schema)
src/resources/  one module per GitHub feature, behind the Resource trait
src/engine/     registry, ordering, plan, apply — knows about no resource
src/github/     the only place that talks to GitHub, behind GitHubClient
src/output/     human and JSON renderers
src/cli/        one module per subcommand
src/diff/       generic keyed-collection diffing, used by the resources
src/schema/     emits the published JSON Schema from the config types
```

Dependencies point inward. A resource that imports from `engine` or `cli` is a
design error.

## When touching configuration types

Run `mise run schema` and commit the result. CI fails otherwise, because the
published schema is a public contract (ADR-007).

## When touching user-facing output

Snapshots will change. Review with `mise run snapshots` and read the diff — the
snapshots exist to make output regressions visible, so rubber-stamping them
defeats the point.

## Documentation that must stay in step

* `docs/authentication.md` — whenever a `Requirement` changes.
* `README.md` — whenever a command, flag or supported setting changes.
* `docs/adr/` — whenever a decision is made or reversed.

## Style

* Rust 2024, stable.
* `miette` at the edges, `thiserror` for typed errors. A diagnostic that
  underlines source text is a `miette` one; everything a caller might match on
  is a typed error.
* Doc comments on configuration types are user-facing: they end up in the
  published schema and in the generated reference.
* Comments explain *why*, not *what*. If a line encodes a GitHub quirk, say
  which quirk.
* Test names are sentences describing behaviour.
