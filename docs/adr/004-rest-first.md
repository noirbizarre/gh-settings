# 4. REST first; GraphQL only as a read accelerator

## Status

Accepted. Supersedes the initial project brief, which preferred GraphQL.

## Context

The brief asked for GraphQL "whenever it provides richer capabilities". For this
domain it almost never does:

* **autolinks** have no GraphQL representation at all;
* **topics**, **rulesets**, **environments**, **variables** and **custom
  properties** have no GraphQL mutations;
* `updateRepository` covers a fraction of what `PATCH /repos` covers — merge
  settings, squash commit options and security features are REST-only.

GraphQL wins in exactly one place: batching several reads into one round trip.

## Decision

REST for everything. GraphQL may later be added as an optional read accelerator
behind the same port, invisible to resources.

## Consequences

* Every setting GitHub exposes is reachable.
* More round trips on read than a batched GraphQL query would need. Mitigated by
  only reading resources the configuration actually manages.
