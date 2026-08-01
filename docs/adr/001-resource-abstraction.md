# 1. Typed `Resource` trait with an object-safe erased wrapper

## Status

Accepted.

## Context

Every GitHub setting has a different shape. Labels are a keyed collection,
repository metadata is a singleton with tri-state fields, rulesets are a nested
tree. The engine needs to hold them all in one list.

A trait with associated `Desired`/`Current` types gives each resource strong
typing and, crucially, makes its `diff` a pure synchronous function that can be
unit tested without a runtime or a network. But associated types make a trait
non-object-safe, so `Vec<Box<dyn Resource>>` is impossible.

The alternative — a uniform `serde_json::Value` based resource — is object safe
and faster to write, but discards type safety exactly where the domain is most
error-prone, and produces poor error messages.

## Decision

Keep the typed `Resource` trait, and recover object safety with a second trait:

```rust
trait Resource { type Desired; type Current; /* ... */ }
trait ErasedResource { /* no associated types */ }
impl<R: Resource> ErasedResource for R { /* blanket */ }
```

The engine only ever sees `Box<dyn ErasedResource>`. Resource authors only ever
implement `Resource`; the blanket impl means nobody writes an `ErasedResource`
impl by hand.

`Resource` also carries an async `prepare` hook, defaulting to the identity. It
exists so `diff` can stay pure: rulesets need to resolve team slugs to numeric
identifiers, which requires the network, and doing that inside `diff` would make
the most test-heavy function in the codebase async.

## Consequences

* Adding a GitHub feature means one module and one registry line.
* `diff` is pure, so the bulk of the test suite needs no runtime and no stub.
* Two traits instead of one, and the blanket impl has to be kept in step when
  the typed trait gains a method.
* `Change::payload` is `serde_json::Value` — the one deliberately untyped seam,
  because it is what makes the plan artifact serialisable.
