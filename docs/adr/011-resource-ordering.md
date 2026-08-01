# 11. Resource ordering via declared dependencies

## Status

Accepted.

## Context

No resource in the first release depends on another. But environments must exist
before their variables, and rulesets may come to reference custom properties.

Retrofitting ordering onto a flat list, after resources have been written
assuming independence, is far more expensive than carrying it from the start.

## Decision

`Resource::depends_on()` returns the resources that must be applied first. The
registry topologically sorts on construction, breaking ties by declaration order
so the registry reads top to bottom the way it executes.

A cycle, or a dependency on an unregistered resource, panics at construction. It
is a programming error in this crate, caught by the test suite, never something
a user can trigger.

## Consequences

* Roughly fifty lines carried before they are needed.
* Adding an ordering constraint later is a one-line change.
* Plan and apply order is deterministic, which keeps snapshots stable.
