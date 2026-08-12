# 15. Token requirements are declared per resource

## Status

Accepted, extended by [020](020-permission-categories-do-not-nest.md).

## Context

The most common failure mode of a tool like this is a `403` that says nothing
useful. The cause is nearly always the credential rather than the configuration.

The `secrets.GITHUB_TOKEN` case is the sharpest: the workflow `permissions:`
block has **no `administration` key**, so repository settings, topics, autolinks
and rulesets cannot be granted to it at all. Users following the obvious Actions
example will hit this immediately.

Documenting permissions in prose guarantees drift between the docs, the
diagnostics and reality.

## Decision

Each resource declares a `Requirement`: fine-grained permissions, classic
scopes, and whether `GITHUB_TOKEN` can manage it at all, with the reason.

That single declaration drives four consumers:

1. the documentation table;
2. the `gh settings doctor` capability table;
3. the hint attached to a `403`;
4. the tests that pin the behaviour.

`doctor` reports honestly. Classic tokens advertise their scopes in a response
header, so those verdicts are exact. Fine-grained and App tokens do not, so
`doctor` probes repository admin rights and otherwise reports **unknown** rather
than guessing. `sync` proceeds regardless and lets the real error speak.

Mappings we could not confirm from first-party documentation are marked
`Confidence::Unverified` in code and confirmed empirically before being asserted.

## Consequences

* The docs cannot drift from the diagnostics.
* Honest uncertainty is a supported outcome, which is better than a confident
  wrong answer.
* Adding a resource means deciding its permissions up front, which is exactly
  when that information is at hand.
