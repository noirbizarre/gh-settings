# Contributing

Thanks for considering it. This project optimises for long-term
maintainability over implementation speed, so expect review to focus on design
and on tests.

## Getting set up

```sh
mise install      # tools: prek, nextest, llvm-cov, insta, typos
mise run          # fmt, lint, build, test
```

`mise` installs a `prek` hook that runs formatting, clippy and commitlint before
each commit.

## Tasks

| Task | What it does |
|---|---|
| `mise run` | Format, lint, build, test |
| `mise run test` | `cargo nextest run` |
| `mise run cover` | Coverage to `lcov.info` |
| `mise run snapshots` | Review pending `insta` snapshots |
| `mise run lint` | Clippy with `-D` warnings |
| `mise run schema` | Regenerate the committed JSON Schema |
| `mise run check` | Everything CI runs |

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

No release automation is configured yet. When it is, it must honour the asset
naming that `gh extension install` requires — see
[ADR-014](docs/adr/014-extension-asset-naming.md); CI already asserts the binary
name on every run.

`cliff.toml` is in place, so `git cliff` produces the changelog and determines
the next version from the commit history in the meantime.
