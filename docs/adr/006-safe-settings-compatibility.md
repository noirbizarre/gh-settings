# 6. `safe-settings` compatibility is one-way and per-repository

## Status

Accepted.

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

Compatibility is **one-way and per-repository**: a `safe-settings` file is read
for the sections we support. Our JSON Schema is our own contract and may extend
beyond theirs.

Concretely, `repository.topics` is accepted as a synonym for the top-level
`topics`. Declaring both is an error rather than an arbitrary precedence.

Organisation-level features — `suborgs`, `overrides`, `.github` inheritance —
are out of scope. An `extends:` mechanism may be revisited later on its own
merits, not as compatibility.

## Consequences

* Migration is a single command for the settings we support.
* We own our schema and can version it properly.
* Users of the organisation-level features of `safe-settings` are not served by
  this tool yet, and the documentation says so plainly.
