# 19. Live-test sandboxes are owned, not created per run

## Status

Accepted. Extends [012](012-testing-strategy.md).

## Context

The stub suite (ADR-012) asserts the shape of the requests we send. It cannot
assert that GitHub accepts them. The live suite closes that gap by running the
real binary against a real repository — which means the tests destroy and
recreate configuration in a repository someone owns.

`Live::preflight()` therefore refuses to start against a repository that already
holds rulesets, autolinks, environments, variables or non-default labels. A
false refusal costs a minute; a false acceptance costs someone their labels.

That refusal decides how sandboxes must be allocated. Two actors sharing one
repository do not interleave — the second one aborts — and a crashed run leaves
the repository dirty for whoever comes next. The workflow's `concurrency` group
serialises CI against itself and against nothing else.

## Decision

**One sandbox per actor, provisioned ahead of time.**

* CI has its own repository, named by the `LIVE_TEST_REPO` variable on the
  `live-test` environment. It is CI's alone.
* Contributors point `GH_SETTINGS_TEST_REPO` at a public repository they own.
* `scripts/live-sandbox.sh` (`mise run test:live:setup`) creates one, or resets
  one a failed run left dirty. It is idempotent and talks to `gh` directly, not
  to gh-settings: a repair tool that depends on the thing being repaired is
  useless on the day it matters.

Rejected: **creating and deleting a repository per run.** It would need
`delete_repo` on a nightly, unattended credential — a token that can destroy
repositories, in exchange for nothing the pre-flight does not already give. A
fresh repository is not usable as-is either: the Pages test builds from a
`gh-pages` branch, so it would still need seeding. And repository creation is
secondary-rate-limited, for a suite that runs once a night.

Rejected: **one repository shared by CI and contributors.** The failure mode is
not a race but a refusal, at 4am, in a job nobody is watching.

## Consequences

* Running the live suite locally takes a one-off setup step. It is one command,
  and it is the same command that recovers from a dirty sandbox.
* The sandbox must be public: on the free plan a private repository answers
  `403 Upgrade to GitHub Pro` for the rulesets endpoints, and rulesets are the
  resource the live suite was written for.
* The reset script must cover more than `Live::cleanup()` does. Cleanup is the
  per-test contract and only purges what the configuration format can prune;
  Pages, the repository fields and the Actions settings survive it, invisible to
  the pre-flight.
* The sandbox must also be *restored* to public, not merely checked. The Actions
  settings behind `/access` and `/fork-pr-workflows-private-repos` exist only on
  a private repository ([ADR-021](021-actions-settings.md)), so the live suite
  flips the sandbox to private and back from a drop guard. A process killed
  outright never runs that guard, and a sandbox stuck private is a sandbox on
  which the rulesets tests cannot run. `scripts/live-sandbox.sh` therefore
  repairs the visibility rather than reporting it — repairing a stuck sandbox is
  what the script is for.
* Nothing enforces the ownership rule mechanically. Pointing the variable at a
  repository that matters will still eat it — which is why the pre-flight
  refuses anything that looks lived-in.
