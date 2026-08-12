# 6. `safe-settings` compatibility is one-way and per-repository

## Status

Accepted. The `extends:` clause below is narrowed by
[ADR-017](017-inheritance.md), which builds the mechanism this deferred.

## Context

`safe-settings` is the incumbent, so its file format is what existing
repositories have. Full compatibility was an initial goal.

But that format is not a specification. It is an accident of Probot passing YAML
straight into `octokit.repos.update()`. It has no version field, no schema, a
branch protection block mirroring a deprecated REST shape, and an organisation
level model (`suborgs`, `overrides`, inheritance from the `.github` repository).

Committing to bidirectional compatibility would mean adopting an unversioned,
organisation-centric format as our own public contract — while simultaneously
promising a formal, versioned JSON Schema. The two cannot both be true.

## Decision

Compatibility is **one-way and per-repository**: the *sections* a `safe-settings`
file uses are read with the same spelling, so they need no rewriting. Our JSON
Schema is our own contract and may extend beyond theirs.

It is not a drop-in read. The schema sets `deny_unknown_fields`, which is what
produces the "did you mean" suggestion on a typo, and an unsupported section —
`branches`, `collaborators`, `teams`, `milestones` — is therefore a parse error
rather than something quietly skipped. Migrating means deleting those sections
first.

Concretely, `repository.topics` is accepted as a synonym for the top-level
`topics`. Declaring both is an error rather than an arbitrary precedence.

Organisation-level features — `suborgs`, `overrides`, `.github` inheritance —
are out of scope. An `extends:` mechanism may be revisited later on its own
merits, not as compatibility. *(It since was: see [ADR-017](017-inheritance.md).)*

## Consequences

* Migration is mechanical for the settings we support, but not automatic: the
  sections we do not support have to be removed before the file will parse.
* We own our schema and can version it properly.
* Users of the organisation-level features of `safe-settings` are not served by
  this tool yet, and the documentation says so plainly.
