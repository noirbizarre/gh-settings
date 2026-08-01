# 13. Single crate until a second consumer exists

## Status

Accepted.

## Context

A workspace splitting the schema and types into a publishable library would let
third parties depend on the configuration model.

There is currently no such third party.

## Decision

A single crate with both `lib.rs` and `main.rs`. The schema is exposed through
`gh settings schema`, which is what tooling actually needs.

## Consequences

* No version coordination between crates, no ceremony.
* If a real second consumer appears, the split is mechanical because the module
  boundaries already reflect it.
