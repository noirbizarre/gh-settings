# 10. The plan is a serialisable artifact; applying re-checks for drift

## Status

Accepted. The fingerprint's inputs are widened by
[ADR-017](017-inheritance.md), which adds the inherited bases.

## Context

The value of a plan/apply split is reviewing a plan and then applying *that*
plan. If `sync` simply recomputes, the artifact is decorative and a change
landing between review and apply goes unnoticed.

## Decision

`plan --out plan.json` writes a versioned artifact. `sync --plan plan.json` reads
it back, then **recomputes** the plan and compares fingerprints. A mismatch is a
hard error telling the user to re-plan.

The fingerprint is derived only from the target and the changes — never from
timestamps — so it is reproducible.

`Change::payload` is `serde_json::Value` precisely so this round trip works
without the engine understanding any resource's data.

## Consequences

* A reviewed plan cannot silently apply something else.
* Applying a saved plan costs a second read of the current state. That is the
  price of the guarantee, and it is worth it.
* The artifact format is a public interface, versioned independently of the CLI.
