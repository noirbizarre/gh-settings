# 22. The release commit is signed by a GitHub App

## Status

Accepted. Narrows [014](014-releases.md), which chose gh-ship and left the
release credentials as a personal access token.

## Context

ADR-014 put `SHIP_TOKEN` — a fine-grained PAT — in a `release` environment,
because the default `GITHUB_TOKEN` cannot trigger the CI that should run on the
Release PR. That solved attribution of the *push* and nothing else. The release
commit itself, the one bumping `Cargo.toml` and rewriting `CHANGELOG.md`,
arrived unsigned and attributed to `github-actions[bot]` by two `git config`
lines copied from an example.

For a project whose entire subject matter is repository configuration, that is
an awkward gap: we can require signed commits of our contributors and not
manage one ourselves.

A commit created on a runner is never signed, whichever token pushes it. Git
signs with a key, there is no key on a runner, and the token only authenticates
the push. Three ways out:

* **Import a signing key.** Verified as a person or an organisation, no API
  round trip — at the cost of a durable private key sitting in secrets, which
  is exactly the kind of long-lived credential ADR-009 says we would rather not
  hold.
* **Let GitHub sign it.** GitHub signs a commit it creates itself on behalf of
  a bot, but only when the request carries no author, committer or signature of
  its own. Adding an identity is what suppresses the signature, so attribution
  and verification are mutually exclusive in a single call. The route is
  therefore: commit and push as usual, then re-create the tip through
  `POST /git/commits` and move the branch onto it. gh-ship ships `gh ship sign`
  to do precisely this.
* **Nothing.** Keep the unsigned commit.

The second requires the token to belong to a **bot**. A fine-grained PAT
belongs to a person, and the API returns the re-created commit unsigned — so
choosing to sign is also choosing to stop using a PAT.

## Decision

**A GitHub App mints an installation token per job, and `gh ship sign`
re-creates the release commit so GitHub signs it.**

`APP_CLIENT_ID` (a variable) and `APP_PRIVATE_KEY` (a secret) live in the
`release` environment. `actions/create-github-app-token` mints a token at the
start of each job and revokes it at the end; what we store is a private key,
useless without a workflow run to use it in.

`SHIP_TOKEN` is gone. `publish-release` does not dispatch workflows and does not
push commits needing CI, so it drops to the default token rather than being
migrated.

At the same time, the deprecated `ship_id` correlation nonce is removed from
both dispatched workflows. gh-ship now correlates a dispatch on the ref, the
event type and run novelty, so neither the input nor the `run-name` stamping it
is used any more.

## Consequences

* The release commit carries **Verified**. Its committer is `GitHub` and its
  author the App's bot user — neither is ours to choose, because choosing is
  what stops GitHub signing. The `git config user.*` lines stay only because a
  runner has no git identity and `git commit` refuses without one; the identity
  they set is discarded with the commit it produces.
* Only the tip is re-created; parents keep their SHAs. The prepare workflow
  makes exactly one commit, so this is total for us, and would silently sign
  only the last if that ever changed.
* `gh ship sign` exits non-zero when the re-created commit comes back unsigned,
  which is what a non-bot token produces. A misconfigured App fails the prepare
  loudly instead of quietly shipping an unsigned release.
* **Installation tokens expire after one hour.** `gh ship prepare` and
  `gh ship release` both block on a dispatched run for up to 60 minutes, so
  every job minting a token caps `timeout-minutes` below that — the release job
  moved from 60 to 50 for this reason alone.
* **The App token cannot go in a job-level `env:`.** It is a step output, and
  the job's `env:` is evaluated before any step runs, so `GH_TOKEN` there is
  the empty string and `gh` falls back to the default token without
  complaining. Every step running `gh` sets it itself, which is more repetition
  than the previous shape and is not an accident.
* `environment: release` becomes load-bearing beyond gating: an environment
  variable is invisible to a job that does not declare its environment, so
  omitting it makes `vars.APP_CLIENT_ID` expand to nothing and the mint step
  fail obscurely.
* Setting up the App is a manual prerequisite, and its installation permissions
  are fixed when it is installed — adding a permission to the App later does
  not grant it to an existing installation until an administrator approves.
  It needs `metadata:read`, `contents:write`, `actions:write`,
  `pull_requests:write`, `issues:write`, and `workflows:write`, the last
  because a release commit here can touch `.github/workflows/`.
