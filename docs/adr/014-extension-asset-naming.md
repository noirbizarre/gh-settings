# 14. Release assets are named for `gh extension install`

## Status

Accepted.

## Context

`gh extension install owner/gh-name` does not build from source. It downloads a
release asset whose name encodes the platform, and expects to find a binary named
after the extension inside it.

Getting either wrong makes the extension silently uninstallable — arguably worse
than publishing no release at all, because the failure surfaces on a user's
machine rather than in our CI.

## Decision

Two constraints are fixed by GitHub, not by preference:

* release assets are named `gh-settings_<version>_<os>-<arch>.{tar.gz,zip}`;
* the binary inside is named `gh-settings`, pinned by `[[bin]] name` in
  `Cargo.toml` with a comment explaining that it cannot be renamed.

CI builds the release binary on every run and asserts both the name and that it
executes, so a rename cannot reach a release unnoticed.

## Consequences

* `gh extension install noirbizarre/gh-settings` works on every supported
  platform.
* The crate name and the binary name differ, which looks like an oversight until
  you know why — hence the comment in `Cargo.toml` and this record.
* **How releases are produced is deliberately left open.** No release automation
  is configured yet; the naming constraint above holds regardless of what is
  eventually chosen, so that decision can be made on its own merits and recorded
  separately.
