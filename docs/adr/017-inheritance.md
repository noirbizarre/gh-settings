# 17. Inheritance replaces whole items, and provenance follows the merge

## Status

Accepted. Narrows the scope clause in
[ADR-006](006-safe-settings-compatibility.md), which deferred `extends:` rather
than forbidding it — *"may be revisited later on its own merits, not as
compatibility"* — and answers the question
[ADR-016](016-diagnostic-provenance.md) left open. Supersedes ADR-016's
resolution rule; see below.

## Context

Sharing labels, autolinks and rulesets across an organisation's repositories is
the most-requested thing this tool cannot do. ADR-016 built the diagnostics
machinery that made it safe to attempt: every span now names the document it
indexes into, so a finding about an inherited file can no longer be rendered
against the local one.

What remained was the merge, and the merge turned out to be constrained by the
data rather than by taste.

## Decision

### Collections replace the whole item, by identity

The roadmap said "merge collections by item identity with the child overriding
field by field". That is not implementable as written. `Label::color` defaults
to `ededed`; `Ruleset::target` and `Ruleset::enforcement` default to `branch`
and `active`. None is an `Option`, so once a document is parsed there is nothing
to distinguish a field the user omitted from one they wrote the default into. A
field-wise merge would repaint an inherited label grey the moment a child
mentioned its name.

So a child item with the same identity replaces the inherited one **outright**.
Identity is the key the diff already uses — labels case-insensitively by name,
autolinks by `key_prefix`, rulesets by `name`, topics by normalised value —
because a merge that disagreed with the diff would produce a plan that creates
something already there.

The cost is real and accepted: adjusting one field of an inherited item means
restating the item. Making those three fields `Option` would remove the
constraint, and is the way to revisit this.

The merge **never deduplicates within a document**. Two labels of the same name
in one file stay two items, so the existing duplicate check still reports the
mistake instead of the merge silently absorbing it.

### `repository` does merge field by field

Every field there *is* an `Option`, so the distinction survives. `Option::or`
is correct for `Nullable<T> = Option<Option<T>>`, where the outer layer means
"is this managed": a child writing `description: null` clears the field rather
than falling back to the inherited value.

`security` recurses rather than being taken whole, or a child enabling
`secret_scanning` would silently unmanage an inherited `advanced_security`.

The struct is destructured exhaustively with no `..`, so a field added later
fails to compile rather than quietly going unmanaged.

### `prune` never inherits

Per [ADR-005](005-prune-opt-in.md). Otherwise editing one shared file starts
deleting across every repository that extends it, decided by someone who does
not own them. `--prune` on the command line still applies to inherited items:
that is an explicit, local instruction, which is the distinction that matters.

### Provenance is resolved through the merge, not alongside it

**This supersedes ADR-016's "resolved by path against the layer stack, last
writer first".** That rule cannot work for collections. Base `[a, b, c]` and
child `[b', d]` merge to `[a, b', c, d]`, in which `labels.1` is the child's item
zero — a positional search across documents would find *a* node and be right
often enough to hide the times it was wrong.

The merge records where every path it produces came from, and that map is the
only lookup. There is deliberately no fallback: a merged configuration has no
default document, so a path the merge forgets to record resolves to nothing and
trips an assertion, rather than being attributed to whichever document happens
to have a node there.

The same mechanism replaced `items_path`, which probed the document to cope with
`Prunable`'s two forms. That probe was already unsound with two documents,
because it asked one question and applied one answer to all of them.

### Single level, and a required ref

A base may not itself extend another: terminating by construction, with no cycle
detection to get wrong. Enforced as a finding carrying the *base's* span, so it
renders against the base's own text.

The ref is required. An unpinned base is a moving target, and a plan reviewed
against one could be applied against a different document with nothing saying
so. `acme/.github@v1` resolves to `acme/.github/.github/settings.yml` — the
repository, then the directory.

### Loading is now a networked operation, conditionally

`validate` was documented as needing no network and no repository. That is now
true only of configurations that do not inherit; the loader is consulted **only**
when the document declares `extends`, and a test asserts that a file without it
issues zero requests.

A base is fetched with the raw media type rather than the contents API's base64
envelope, which would have meant a dependency to work around a limitation that
was one branch away from not existing.

### A moved base is not drift

`sync --plan` recomputes and compares. Once a base is shared, the likeliest
cause of a mismatch is that the base moved — and reporting that as drift sends
people to inspect a repository that has not changed, usually their own, while
the base belongs to someone else. The plan records each base and the commit it
was read at, taken from the ETag the fetch already returns.

## Consequences

* A finding about an inherited document is rendered against that document, under
  its own name, beside findings about the local file.
* `extends` requires `Contents: read` on the *other* repository. The Actions
  `GITHUB_TOKEN` cannot do this — the same class of dead end as
  `Administration: write` — so `doctor` says so, and only when it applies.
* `export` still flattens. It dumps live state and never reads the configuration
  file, so re-exporting an inheriting repository re-declares everything locally
  and loses the inheritance. Its header already warns that exports describe
  current state rather than intent; subtracting inherited values would mean
  `export` could no longer run on a repository with no configuration at all.
* The saved plan format gains a defaulted field and stays at version 1.
* This narrows [ADR-010](010-plan-artifact.md), which says the fingerprint is
  derived "only from the target and the changes". It now also covers each
  inherited base and the commit it was read at, so a base that moved makes a
  different plan even when the resulting changes happen to coincide.
* Two shipped bugs were found on the way and fixed first, because the merge
  would have been built on top of them: ruleset findings underlined whichever
  rule sorted into the position, and a configuration using only
  `repository.topics` panicked a debug build.

### Open

Per-item provenance assumes each merged item came from exactly one document,
which replace-by-identity guarantees and concatenation would not. Revisiting the
merge semantics means revisiting this.
