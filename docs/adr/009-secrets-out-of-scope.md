# 9. Secrets are out of scope

## Status

Accepted.

## Context

Managing Actions secrets declaratively is an obvious request, and the initial
scope list included it.

GitHub secrets are write-only. The API exposes their names but never their
values. That has three consequences:

* they cannot be diffed — we cannot know whether a value differs;
* they cannot be exported — there is nothing to read back;
* they cannot be idempotent — writing the same value always reports a change.

A `secrets:` block in a committed YAML file is also an obvious footgun.

## Decision

Secrets are out of scope, and the documentation explains why rather than leaving
it as an omission. Users are pointed at `gh secret set`.

Actions **variables** are not affected: their values are readable and they will
be supported.

## Consequences

* The idempotency guarantee holds without exceptions.
* No mechanism exists to encourage putting secret material into a repository.
* A frequently requested feature is declined. Name-only management (ensuring a
  secret exists without touching its value) remains a possibility if there is
  demand.
