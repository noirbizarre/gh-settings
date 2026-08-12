# 18. A resource may span more than one configuration section

## Status

Accepted.

## Context

Actions variables exist at two scopes: repository-wide, and scoped to a single
deployment environment. They are the same thing at both — identical payload,
identical verbs, identical normalisation, identical diff — differing only in
which collection endpoint they hang off.

The configuration, however, reads best when they are nested where they belong:

```yaml
environments:
  - name: production
    variables:
      - name: DEPLOY_URL
        value: https://example.com

variables:
  - name: DEFAULT_REGION
    value: eu-west-1
```

Until now every resource owned exactly one top-level section, and the engine
relies on that in one place: `export` files each resource's output under
`resource.id().as_str()`.

Two obvious shapes were rejected.

**A second resource for environment variables** would duplicate `diff` and
`apply` wholesale to change a path prefix, and would need a name nobody would
guess (`environment-variables`?). Worse, `--only variables` would then quietly
do half the work — a plan that silently omits changes is the worst failure mode
this tool has.

**A flat `variables:` list with an `environment:` field on each item** keeps one
resource but makes the file read nothing like the product it describes, and
makes "which environments does this file manage?" a question you answer by
scanning a list.

## Decision

A resource may read from, and write to, more than one configuration section. The
`variables` resource owns both `variables` and `environments[].variables`.

Three rules follow, and none of them is optional:

1. **`desired()` must consult every section it owns.** `is_managed` is
   `desired().is_some()`, and an unmanaged resource is skipped entirely — so a
   resource that looked only at its namesake section would silently never write
   anything for a file that used only the nested one.

2. **Pruning is scoped.** A scope the configuration says nothing about is never
   pruned, whatever `--prune` says. `variables: {prune: true}` asks to tidy the
   repository's own variables, not every environment's. Environment variables
   are pruned under the `environments` section's flag instead, because a
   variable cannot outlive the environment holding it.

3. **`export` is split along section lines, not resource lines.** Each section
   is emitted by the resource whose identifier names it: `variables` exports the
   repository scope, and `environments` exports the nested lists as part of the
   environments it is already emitting.

Nested collections are plain lists, not `Prunable`. A per-item `prune` flag
would have no meaning distinct from its section's, and the provenance index only
rewrites the `items` indirection for top-level sections — a nested `Prunable`
would therefore underline the wrong node in a diagnostic.

## Consequences

* One diff, one apply path and one `--only` name for a feature that GitHub
  exposes at two scopes.
* `export` reads environment variables twice, once per resource. That is N
  additional `GET`s on a command that is run rarely and interactively, and it
  buys a `current()` that does not have to know about the configuration.
* The invariant "one resource, one section" is gone, and with it the temptation
  to "fix" the export split back into a bug. Both module headers say so.
* `Provenance::resolve` already rewrites the longest recorded prefix and appends
  the remainder, so `environments.3.variables.0.name` resolves through the
  recorded `environments.3` entry with no new machinery. This is load-bearing
  and is covered by a test that underlines a nested variable name.
* Ordering matters for the first time: `variables` declares `depends_on`
  `environments` ([ADR-011](011-resource-ordering.md)). Since planning completes
  before any apply, reading an environment that does not exist yet 404s, and
  that is read as "no variables" rather than as an error.
