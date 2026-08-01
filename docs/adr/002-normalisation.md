# 2. Normalisation is mandatory and resource-owned

## Status

Accepted.

## Context

GitHub silently rewrites what it is given:

* topics are lowercased and their separators normalised to hyphens;
* label colours lose their `#` and are lowercased;
* an unset description is reported as `""`, not `null`;
* autolinks default `is_alphanumeric` to `true`;
* rulesets return `id`, `created_at`, `_links` and arbitrarily ordered rules.

Comparing raw configuration against a raw API response therefore produces a
*permanent diff*: `plan` reports the same change forever and `sync` is never
idempotent. This is the single most likely way for the tool to become useless.

## Decision

Every resource normalises **both sides** before diffing. Normalisation lives with
the resource, not in the engine, because only the resource knows its field
semantics.

Every resource is covered by tests that assert the specific traps above, written
as behaviour ("a case-only colour difference is not a change") rather than as
implementation detail.

## Consequences

* `sync` run twice produces no changes, which is the headline promise.
* Each new resource carries an obligation to identify its own traps; a missing
  normalisation shows up as a plan that never converges.
* Some information is deliberately discarded during comparison, so a resource
  cannot detect a change in a field it normalises away.
