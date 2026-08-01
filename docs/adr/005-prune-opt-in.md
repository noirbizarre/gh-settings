# 5. Pruning is opt-in; deletions are always explicit

## Status

Accepted.

## Context

A declarative tool must decide whether the configuration is *authoritative*
(anything not listed is deleted) or *additive* (only listed things are managed).
There is no state file to consult, and no third option.

`safe-settings` is authoritative and does delete labels. That behaviour is
precisely why people are wary of adopting it: the first run against an
established repository can destroy years of accumulated configuration.

## Decision

Pruning is **off by default**, and opt-in per resource:

```yaml
labels:
  prune: true
  items: [...]
```

`--prune` and `--no-prune` override the configuration in both directions, so an
operator can always force safety regardless of what the file says.

Deletions always appear in the plan as explicit `- delete` lines, and a plan
containing any destructive change prints a warning and requires confirmation.

## Consequences

* Adopting the tool on an existing repository is safe by construction. This is
  the property that makes migration plausible.
* We diverge from `safe-settings`, so a migrated configuration will not prune
  until pruning is explicitly enabled. Documented in the migration guide.
* Reaching a truly declarative state requires a deliberate opt-in, which is the
  correct place for that decision to be made.
