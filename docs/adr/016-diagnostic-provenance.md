# 16. Diagnostics carry the identity of the document they came from

## Status

Accepted. Extends [ADR-008](008-yaml-spans.md), which established the span index
but assumed a single source document.

## Context

A `miette::SourceSpan` is a byte offset and a length. Nothing more. That is
sufficient while a configuration is exactly one file, and it fails in a
particular way when it is not: an offset computed against one document is still
a perfectly *valid* index into another. Rendering a span against the wrong text
therefore produces a confident underline beneath unrelated characters, rather
than an error. It is wrong in the one way a diagnostic must never be — plausibly.

[ADR-006](006-safe-settings-compatibility.md) deferred an `extends:` mechanism
rather than forbidding it, and it remains the most-requested unbuilt feature.
Merging settings from a second document makes the above inevitable.

The problem was not theoretical when this was written. `SpanIndex::resolve`
walked up to the nearest ancestor and finally to the document root, so it could
never return "no". Validation asked for `labels.0.color`, which is correct for
`labels: [...]` and wrong for `labels: { prune: true, items: [...] }`, and the
miss silently widened into an underline covering the whole section. One file,
same failure mode, shipped.

## Decision

**A span is meaningless without the identity of the text it indexes into, so the
two travel together.**

`FileSpan { source: SourceId, span: SourceSpan }` replaces the bare span on
`Finding`. `Sources` owns the documents; `SourceId` is an index into it.

An index rather than an `Arc<NamedSource>` or a path. A finding is *data* —
cloned, sorted, serialised — while the document text is *rendering context*
needed once, at the end. A path is not an identity either: two inheritance
chains can reach the same file, and a document fetched from another repository
has no local path.

**Path lookup is split by intent.** `resolve` keeps the ancestor fallback, which
`serde_path_to_error` genuinely requires — it reports paths one level deeper
than any node present, such as a missing `repository.description` when only
`repository` exists. `exact` has no fallback, and hand-written validation uses
it: a path that matches nothing is a bug in the caller, and quietly widening it
to the enclosing section is what let that bug survive. `ValidateCtx` asserts on
the miss in debug builds.

**Provenance is resolved by path against the layer stack, last writer first.**
It cannot be recovered from the merged `Settings`, which has no memory of where
a value came from. Whichever document last declared a path is the one whose
value ended up in the settings, so it is the one to underline. The merge and the
lookup then share a single rule, which makes a divergence between them a
testable property rather than a mystery.

**Rendering follows miette's grain.** A `Diagnostic` resolves every label
against one `source_code`, so a report covering several documents *is* several
diagnostics: findings from non-root documents are grouped into sub-reports
surfaced through `Diagnostic::related`, each owning its own text and announcing
its own severity.

### Settled, for the `extends:` record that follows

* Inherit from **another repository**, ref-pinnable (`acme/.github@v1`). No
  local-path form: sharing configuration across repositories is the use case,
  and a local include solves a problem nobody has.
* **Single level.** A base file may not itself extend another. Terminating by
  construction, with no cycle detection to get wrong.
* **`prune` never inherits** ([ADR-005](005-prune-opt-in.md)). Otherwise editing
  one shared file starts deleting across every repository that extends it,
  decided by someone who does not own them.

## Consequences

* A finding can no longer hold an offset without saying what it indexes into.
  The failure this record exists to prevent is now unrepresentable rather than
  merely avoided.
* With one document, `related()` is empty and rendering is byte-identical.
  Every existing snapshot was unchanged by the refactor, which is what made a
  30-call-site change reviewable.
* The JSON output gains `file` alongside `offset`. Additive, per
  [ADR-007](007-schema-is-the-contract.md).
* `Config` holds a `Sources` and a `Vec<SpanIndex>` that are plural in shape and
  singular in fact until `extends:` lands. That is deliberate: the cost is a
  little ceremony now against a much larger change later.
* Span paths are still built by `format!` from positional indices and are still
  unchecked at compile time. The debug assertion and the both-forms tests catch
  the common failures; a path builder that can only descend from nodes that
  exist would remove the class entirely, and is the intended direction. It would
  rewrite all thirty call sites, so it is not this change.

### Open, for the `extends:` record

Per-*item* provenance inside a merged collection. If two documents each declare
labels, item *n* of the merged list may come from either, and which one depends
on the list merge semantics — replace, concatenate, or merge by item identity.
That decision belongs with the merge, not here. Until it is made, a finding
about a merged list item resolves to whichever document last declared the
enclosing path, which is right for override and wrong for concatenation.
