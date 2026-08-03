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

- [x] **A live test suite** against a real throwaway repository, `#[ignore]`d
      and gated on `GH_SETTINGS_TEST_REPO`. Nine tests covering each resource's
      create → update → prune cycle, re-planning after every mutation. Runs
      nightly and on demand; refuses to start against a repository that already
      has managed configuration. Run it yourself with
      `GH_SETTINGS_TEST_REPO=you/sandbox mise run test:live` — the sandbox must
      be **public**, since a private repository on the free plan answers
      `403 Upgrade to GitHub Pro` for rulesets.
- [ ] **Ruleset apply-path tests through the stub.** Create, update, delete and
      prune, asserting the request log. Manually verified against the real API
      once; nothing yet stops a regression.
- [x] **Repository security PATCH test** — covered by the live suite.
- [x] **Generic idempotency contract** — the live suite re-plans after every
      mutation and asserts the plan is empty, for every resource. Checked
      against reality rather than against our own fixtures, which is the only
      version of this assertion that would have caught the ruleset permanent
      diff.
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
- [ ] **Help with ruleset rule parameters.** GitHub requires *all* parameters of
      a rule, not the subset you want to change: a `pull_request` rule missing
      one field is rejected with `Invalid property /rules/1: data matches no
      possible input`, which names neither the rule nor the field. Validation
      could name the rule from the index, and list the parameters a known rule
      type expects.

### Features

- [x] **A composite action** — `uses: noirbizarre/gh-settings@v1`. Maps exit
      code 2 to a `changed` output rather than a failed job, writes the plan to
      the job summary, and annotates a 403 with the token explanation. See
      [GitHub Actions](actions.md).

- [ ] **Inheritance (`extends:`)** — share labels, autolinks and rulesets across
      repositories. Decided: inherit from another repository, ref-pinnable
      (`acme/.github@v1`); merge collections by item identity with the child
      overriding field by field; **`prune` never inherits**, because otherwise
      editing one shared file would start deleting across every repository that
      extends it, decided by someone who does not own them.

      The load-bearing risk is not the merge, it is the diagnostics. A
      `SourceSpan` is a byte offset with no file identity, so an offset from the
      base file is still a *valid* index into the local one: a finding about the
      shared file would render a confident underline pointing at unrelated text.
      It fails silently rather than erroring. `Finding`, `Report`, `SpanIndex`
      and `ValidateCtx` all assume a single source today and need provenance
      before this ships.

      Needs its own ADR. [ADR-006](adr/006-safe-settings-compatibility.md)
      deferred `extends:` rather than forbidding it — *"may be revisited later
      on its own merits, not as compatibility"* — so this narrows that scope
      clause rather than reversing the record.

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
- [x] Sync a **ruleset** with rules and `conditions.ref_name` — found a `422`;
      GitHub requires *every* parameter of a `pull_request` rule, not a subset
- [x] Update that ruleset (change a rule parameter) and re-sync — exercises
      `PUT /rulesets/{id}` and canonical rule ordering
- [ ] A ruleset with `bypass_actors: [{ team: … }]` **on an organisation
      repository** — slug-to-id resolution; impossible to test on a personal repo
- [x] `repository.security.secret_scanning: true`
- [x] Change an autolink's `url_template` and re-sync — the recreate path
- [x] `sync --prune` after removing a label — the destructive path and its prompt
- [x] Run any `sync` **twice**; the second must report "up to date" — this is
      what caught the ruleset permanent diff
- [ ] `doctor` inside GitHub Actions with `secrets.GITHUB_TOKEN` — should report
      labels-only and say why
- [ ] `gh extension install noirbizarre/gh-settings` on a clean machine

A `422` means a payload-shape bug, and GitHub will not say which field.
