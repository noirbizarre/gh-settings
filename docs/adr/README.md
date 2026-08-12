# Architecture Decision Records

Each record states a decision, the forces behind it, and what it costs. They are
immutable once accepted: a decision that turns out badly gets a new record that
supersedes it, rather than a rewrite of history.

| # | Decision | Status |
|---|---|---|
| [001](001-resource-abstraction.md) | Typed `Resource` trait with an object-safe erased wrapper | Accepted |
| [002](002-normalisation.md) | Normalisation is mandatory and resource-owned | Accepted |
| [003](003-gh-api-transport.md) | GitHub access shells out to `gh api` behind a port | Accepted |
| [004](004-rest-first.md) | REST first; GraphQL only as a read accelerator | Accepted |
| [005](005-prune-opt-in.md) | Pruning is opt-in; deletions are always explicit | Accepted |
| [006](006-safe-settings-compatibility.md) | `safe-settings` compatibility is one-way and per-repository | Accepted, narrowed by 017 |
| [007](007-schema-is-the-contract.md) | The JSON Schema is the public contract, generated from Rust | Accepted |
| [008](008-yaml-spans.md) | YAML spans come from a side index for precise diagnostics | Accepted |
| [009](009-secrets-out-of-scope.md) | Secrets are out of scope | Accepted |
| [010](010-plan-artifact.md) | The plan is a serialisable artifact; applying re-checks for drift | Accepted, narrowed by 017 |
| [011](011-resource-ordering.md) | Resource ordering via declared dependencies | Accepted |
| [012](012-testing-strategy.md) | Testing through a `gh` process stub and request-log assertions | Accepted, extended by 019 |
| [013](013-single-crate.md) | Single crate until a second consumer exists | Accepted |
| [014](014-releases.md) | Releases are orchestrated by gh-ship | Accepted |
| [015](015-token-requirements.md) | Token requirements are declared per resource | Accepted, extended by 020 |
| [016](016-diagnostic-provenance.md) | Diagnostics carry the identity of the document they came from | Accepted |
| [017](017-inheritance.md) | Inheritance replaces whole items, and provenance follows the merge | Accepted |
| [018](018-resources-may-span-sections.md) | A resource may span more than one configuration section | Accepted |
| [019](019-live-test-sandboxes.md) | Live-test sandboxes are owned, not created per run | Accepted |
| [020](020-permission-categories-do-not-nest.md) | Fine-grained permission categories do not nest | Accepted |
