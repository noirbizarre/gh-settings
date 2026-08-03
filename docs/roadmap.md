# Roadmap

Where `gh-settings` is, where it is going, and what it will deliberately never
do. Decisions already settled live in the [architecture decision
records](adr/README.md); this page tracks what is left.

The guiding ambition: *if it is under the repository Settings page, it should
eventually be manageable here.*

---

## Supported settings

| Resource | Status | Notes |
|---|---|---|
| Repository metadata | ✅ | description, homepage, features, merge strategies, commit title/message, default branch |
| Repository security | ✅ | secret scanning, push protection, Dependabot updates, advanced security |
| Topics | ✅ | |
| Labels | ✅ | including renames, which preserve issue assignments |
| Autolinks | ✅ | changes are delete-and-recreate; GitHub has no update endpoint |
| Rulesets | ✅ | unknown rule types round-trip untouched rather than being dropped |
| Custom properties | planned | |
| Environments & variables | planned | variables are readable, so they can be diffed |
| Webhooks | planned | |
| Pages | planned | |
| Collaborators & teams | planned | |
| Branch protection (legacy) | planned | rulesets are the modern equivalent and already supported |
| Repository interactions | planned | |
| Secrets | ✖ never | [ADR-009](adr/009-secrets-out-of-scope.md) — values are write-only, so they cannot be diffed, exported, or made idempotent |

Every section of the configuration file is optional, and **an absent section is
unmanaged**. Adoption is incremental: you can manage labels alone and nothing
else will move.

---

## Next

Roughly in priority order. Each entry says *why*, so it can be dropped when the
reason stops applying.

### Verification

- [ ] **Ruleset apply-path tests.** Rulesets are the most intricate resource and
      have **zero** integration coverage of create/update/delete, and have never
      run against the real API. Any payload-shape bug surfaces as a bare `422`
      that does not say which field is wrong.
- [ ] **Repository security PATCH test.** The `security_and_analysis`
      sub-object travels in its own request with a `{status: …}` shape, and that
      split is untested end to end.
- [ ] **Generic idempotency contract test** over the registry: apply, re-plan,
      assert empty — for every resource, not just labels. A missed normalisation
      is the failure that makes the tool useless, and it currently has only
      ad-hoc coverage.
- [ ] **Snapshots for `plan`, `doctor` and `export`.** Only `validate` output is
      snapshotted, so the other three can regress silently.
- [ ] **`--continue-on-error` integration test.** Implemented and unit-tested,
      never exercised through the binary.

### Behaviour

- [ ] **`sync` pre-flight permission check.** Today a permission problem is
      discovered only *after* a failed write. The `Requirement` data already
      exists ([ADR-015](adr/015-token-requirements.md)); consulting it up front
      lets `sync` fail fast and name the missing permission. Must stay
      conservative — refuse only when certain, and let an unknown token proceed
      so the real error can speak.
- [ ] **Attach the requirement table to a `403`**, rather than the current
      generic "run `doctor`" hint.

### Documentation

- [ ] Installation page with per-platform notes and upgrading.
- [ ] Quick start: export → validate → plan → sync on an existing repository.
- [ ] Migration guide from `safe-settings` — what is read, what is not
      (org-level `suborgs`/`overrides`), and above all that **pruning is off by
      default**, so the first run is non-destructive. That difference is the
      main reason to migrate and is currently buried in an ADR.
- [ ] FAQ: why not safe-settings, why no secrets, why a `403`, why a PAT in CI.

### Infrastructure

- [ ] **Codecov token.** Uploads are currently tokenless; a rate-limited leg
      leaves the coverage status *pending* rather than failed, which reads as
      "still running" instead of "broken" — the worse failure mode.
- [ ] **Pin `gh-ship`** in the release workflows. It is installed unpinned and
      executed inside the privileged `release` environment, while every other
      tool is pinned through `mise.lock`. The inconsistency, not the trust, is
      the problem.
- [ ] **`github/resolver.rs` is dead code.** Rulesets re-implement its caching
      inline. Either use it in `Resource::prepare()`, which is where it belongs,
      or delete it — an orphaned port that the ADRs describe as load-bearing is
      worse than neither.
- [ ] Harden `${{ inputs.tag }}` interpolation in `publish-release.yaml` by
      passing through `env:`. Only reachable by someone who already has write
      access, so defence in depth rather than a hole.

---

## Not planned, and why

These are settled. Reopening one means superseding its record, not re-arguing
it.

| | Decision |
|---|---|
| **Secrets** | [ADR-009](adr/009-secrets-out-of-scope.md) — write-only values cannot be diffed, exported or made idempotent. Use `gh secret set`. |
| **Org-level `suborgs` / `overrides`** | [ADR-006](adr/006-safe-settings-compatibility.md) — `safe-settings` compatibility is one-way and per-repository. An `extends:` mechanism may return later on its own merits, not as compatibility. |
| **A `diff` command** | `plan --verbose` already shows field-level before/after; a second command for the same output is surface without substance. |
| **A typed enum of ruleset rules** | GitHub ships new rule types faster than any client tracks them. The untyped passthrough is deliberate: an unrecognised rule round-trips untouched rather than being silently dropped on the next sync. |
| **GraphQL** | [ADR-004](adr/004-rest-first.md) — autolinks, topics, rulesets, environments and custom properties have no GraphQL mutations at all. |
| **Publishing to crates.io** | [ADR-014](adr/014-releases.md) — `gh-settings` is distributed as a GitHub CLI extension; a second install path is a second thing to keep working. |

---

## Manual verification checklist

Automated tests assert the *shape* of the requests we send. They cannot assert
that GitHub *accepts* them, so these paths need a human and a throwaway
repository. Several are destructive.

- [x] Add a topic
- [x] `export`
- [x] `plan`
- [x] `sync`
- [x] `export` → `plan` reports zero changes
- [ ] Sync a **ruleset** with rules and `conditions.ref_name` — never run against
      the real API
- [ ] Update that ruleset (change a rule parameter) and re-sync — exercises
      `PUT /rulesets/{id}` and canonical rule ordering
- [ ] A ruleset with `bypass_actors: [{ team: … }]` **on an organisation
      repository** — slug-to-id resolution; impossible to test on a personal repo
- [ ] `repository.security.secret_scanning: true`
- [ ] Change an autolink's `url_template` and re-sync — the recreate path
- [ ] `sync --prune` after removing a label — the destructive path and its prompt
- [ ] Run any `sync` **twice**; the second must report "up to date"
- [ ] `doctor` inside GitHub Actions with `secrets.GITHUB_TOKEN` — should report
      labels-only and say why
- [ ] `gh extension install noirbizarre/gh-settings` on a clean machine

A `422` means a payload-shape bug, and GitHub will not say which field.
