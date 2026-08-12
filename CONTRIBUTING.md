# Contributing

Thanks for considering it. This project optimises for long-term
maintainability over implementation speed, so expect review to focus on design
and on tests.

## Getting set up

```sh
mise install      # tools, pinned by mise.lock
mise run          # fmt, lint, lint:actions, build, test
```

Tool versions are resolved to exact builds in `mise.lock`, which is committed.
CI installs from the same two files, so a tool release cannot change CI
behaviour on its own.

`mise` installs a `prek` hook that runs formatting, clippy and commitlint before
each commit.

## Tasks

| Task | What it does |
|---|---|
| `mise run` | Format, lint, lint the workflows, build, test |
| `mise run test` | `cargo nextest run` (accepts nextest selectors) |
| `mise run test:live` | The live suite, against `GH_SETTINGS_TEST_REPO` |
| `mise run test:live:setup` | Create or repair a sandbox for the live suite |
| `mise run cover` | Coverage via `cargo llvm-cov` |
| `mise run snapshots` | Review pending `insta` snapshots |
| `mise run lint` | Clippy with `-D warnings` |
| `mise run lint:actions` | `actionlint` over the workflows |
| `mise run spell` | `typos` |
| `mise run schema` | Regenerate the committed JSON Schema |
| `mise run schema:check` | Fail if the committed schema is stale (what CI runs) |
| `mise run docs:reference` | Regenerate the configuration reference from it |
| `mise run docs` | Serve the documentation locally |
| `mise run dogfood` | `gh ship validate` — check the release setup |

## Adding support for a new GitHub setting

This is the path the architecture is designed for, and it should not require
touching the engine.

1. Create `src/resources/<name>/` with `mod.rs`, `model.rs` and `tests.rs`.
2. Implement the `Resource` trait. The associated `Desired` and `Current` types
   are yours to shape.
3. **Write `normalized()` first.** GitHub rewrites what you send it — lowercasing,
   defaulting, reordering, adding server-only fields. Anything you fail to
   normalise becomes a diff that never converges. See
   [ADR-002](docs/adr/002-normalisation.md) for the traps found so far.
4. Declare a `Requirement`. This single declaration drives the documentation
   table, `gh settings doctor`, and the hint attached to a `403`
   ([ADR-015](docs/adr/015-token-requirements.md)). If you cannot confirm a
   fine-grained permission mapping from GitHub's own reference, mark it
   `Confidence::Unverified` rather than guessing.
5. Add a variant to `ResourceId` and one line to `Registry::default()`.
6. Add the configuration section to `config::settings::Settings`.
7. Run `mise run schema` and commit the regenerated schema.

### What review will ask about

* **Is `diff` pure?** It must be synchronous and total. Anything needing the
  network belongs in `prepare()`.
* **Does an omitted field stay unmanaged?** Absence must never be read as
  "reset to default". This is what makes partial configuration files safe.
* **Is deletion opt-in?** Pruning defaults to off
  ([ADR-005](docs/adr/005-prune-opt-in.md)).
* **Is it idempotent?** Add a test proving that applying then re-planning yields
  nothing.
* **Are the tests about behaviour?** `a_case_only_colour_difference_is_not_a_change`
  tells a reader why the code exists; `test_normalize_2` does not.

## Testing

Three layers, described in [ADR-012](docs/adr/012-testing-strategy.md):

* **Unit tests**, inline under `#[cfg(test)]`. `normalize` and `diff` are pure,
  so this is where most coverage should live — no runtime, no stub, no network.
* **Integration tests** in `tests/`, driving the real binary against a stub `gh`
  on `PATH`. Assert on `output.writes()` — *which* requests were issued, in
  *which* order — not only on what was printed.
* **Snapshot tests** with `insta`, for diagnostics and plans. Review with
  `mise run snapshots`.

The stub answers unregistered reads with an empty result but *fails*
unregistered writes, so an unexpected mutation cannot slip through unnoticed.

### The live suite

A fourth, optional layer runs the real binary against a real repository
([ADR-019](docs/adr/019-live-test-sandboxes.md)). It is `#[ignore]`d and skips
unless `GH_SETTINGS_TEST_REPO` names one, so an ordinary `mise run` never
touches the network.

**Bring your own sandbox.** The repository CI uses belongs to CI. The tests
mutate whatever they are pointed at, and the pre-flight refuses a repository
that already holds managed configuration — so two people sharing one do not
interleave, they make each other's run abort.

```sh
mise run test:live:setup you/gh-settings-sandbox
export GH_SETTINGS_TEST_REPO=you/gh-settings-sandbox
mise run test:live
```

The sandbox must be **public**: on the free plan a private repository answers
`403 Upgrade to GitHub Pro` for the rulesets endpoints, which is most of what
the suite is for. To keep the variable across shells, put it in
`mise.local.toml` (git-ignored):

```toml
[env]
GH_SETTINGS_TEST_REPO = "you/gh-settings-sandbox"
```

Each test cleans up after itself from a destructor, so a failing test leaves the
sandbox as it found it. A run that is *killed* — cancelled, timed out, OOM —
runs no destructor, and the next run will then refuse to start:

```
refusing to run the live suite: you/sandbox already has rulesets
```

That is the safety check working, not a bug. `mise run test:live:setup --yes`
resets the sandbox — it goes wider than the tests' own cleanup, and it reports
what it could not reset rather than stopping at the first refusal.

Pages is the one thing neither can remove. On a public repository GitHub ties
the site to the `gh-pages` branch and refuses to deactivate it while that branch
exists — and the sandbox needs the branch, so a Pages site there is expected.

One test, `live_declared_permissions_match_what_github_accepts`, asks GitHub
what permissions each endpoint really requires and checks our declarations
against the answer. GitHub only answers fine-grained tokens, so it prints a skip
and passes on anything else. A skip there is expected, not a broken suite.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/), enforced by
commitlint. The changelog and the next version number are derived from them, so
the type and scope matter:

```
feat(labels): support renaming via new_name
fix(topics): normalise underscores to hyphens
docs(adr): record why secrets are out of scope
```

## Architecture decisions

Anything that will still be visible in five years — a new dependency, a change
to the resource abstraction, a change to the configuration format — wants an
ADR in `docs/adr/`. Records are immutable: supersede rather than rewrite.

## Releases

Orchestrated by [gh-ship](https://github.com/noirbizarre/gh-ship), which is why
commit messages matter: `git cliff` derives both the changelog and the next
version number from them.

The lifecycle is:

1. push to `main` → `gh ship prepare` opens or updates the **Release PR**,
   carrying the version bump and the changelog;
2. review the changelog and merge it;
3. `gh ship release` tags the merge commit, drafts the release, attaches the
   cross-compiled binaries, and only then makes it public.

Maintainers do not tag by hand. `gh ship validate` runs in CI, so a workflow
that stops satisfying the contract fails on a pull request rather than
mid-release. See [ADR-014](docs/adr/014-releases.md).
