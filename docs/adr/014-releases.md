# 14. Releases are orchestrated by gh-ship

## Status

Accepted. Supersedes the placeholder recorded when this project was first
scaffolded, which deferred the choice of release automation.

## Context

Two constraints are fixed by GitHub rather than by preference:

* `gh extension install owner/gh-name` does not build from source. It downloads
  a release asset whose name encodes the platform, and expects a binary named
  after the extension inside it. Getting either wrong makes the extension
  silently uninstallable — worse than publishing nothing, because the failure
  surfaces on a user's machine rather than in our CI.
* the default `GITHUB_TOKEN` cannot trigger workflows, so a Release PR it
  authors shows no CI results.

Beyond that, a release needs someone to decide *when*, to stage the changes for
review, to tag the right commit, and to get the binaries attached before anyone
is notified.

Most release tools want to own all of it, including versioning and changelog
generation. That is precisely the part this project already has opinions about:
Conventional Commits and git-cliff.

## Decision

Releases are orchestrated by [gh-ship](https://github.com/noirbizarre/gh-ship),
which splits the work along that seam:

| gh-ship does | our workflow does |
|---|---|
| create the release branch | bump the version |
| dispatch workflows and correlate runs | generate the changelog with git-cliff |
| validate the release artifact | regenerate the JSON Schema |
| render and maintain the Release PR | commit and push |
| tag, draft, publish assets, then reveal | |

The integration surface is one JSON document, `ship.release.json`, uploaded as
an artifact named `ship-release`. gh-ship never learns how we version and never
generates a changelog.

Concretely:

* `.github/ship.yml` configures the orchestration;
* `prepare-release.yaml` derives the next version with `git cliff
  --bumped-version`, writes the changelog, bumps `Cargo.toml`, regenerates the
  committed JSON Schema, and emits the artifact;
* `publish-release.yaml` cross-compiles six targets and attaches them, named
  `gh-settings_<tag>_<os>-<arch>`, to the release gh-ship has already created as
  a **draft**;
* `ship.yaml` drives both halves: `gh ship prepare` on push to main, `gh ship
  release` when the Release PR merges.

Releases are drafted first so the assets land before anyone is notified. An
extension announced before its binaries exist is an extension nobody can
install.

`SHIP_TOKEN` lives in a `release` environment, because `GITHUB_TOKEN` cannot
trigger the CI that should run on the Release PR.

## Consequences

* Versioning and changelog stay ours, expressed in `cliff.toml`, which is kept
  deliberately identical to gh-ship's so the two extensions read the same way.
* `gh ship validate` runs in CI, so a workflow that stops satisfying the
  contract fails on a pull request rather than mid-release.
* The binary name is fixed by an external constraint, not by preference — hence
  the comment on `[[bin]]` in `Cargo.toml` and the CI job that asserts it.
* We depend on another young extension of our own. That is deliberate
  dogfooding; the fallback, a hand-written workflow, is well understood and the
  protocol is a single JSON file, so the escape hatch stays cheap.
* Requires a `release` environment holding `SHIP_TOKEN` before the first
  release. Without it the workflows fall back to `GITHUB_TOKEN` and the Release
  PR gets no CI.
